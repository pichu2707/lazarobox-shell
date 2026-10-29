//! The OpenAI-style tool decoder: `delta.tool_calls` fragments of an SSE
//! stream, assembled per `index` and released through the same `tool_call()`
//! the Ollama decoder uses.

use serde_json::{Value, json};

use super::{
    CallError, MAX_CALLS, SERVER_LINE_CAP, SseParser, StreamDecoder, StreamEvent, ToolCalls,
    ToolRequest, refused, too_long, tool_call,
};
use crate::core::ai::tools::{registry::FamilySet, servers::Qualified};

/// One call being assembled.
struct Slot {
    index: Option<u64>,
    id: String,
    name: String,
    arguments: String,
    /// A `name` or `arguments` that was not a string: this call is refused.
    broken: bool,
}

/// A string field; anything but `null` or absent marks the call broken.
fn text<'a>(value: &'a Value, broken: &mut bool) -> &'a str {
    match value {
        Value::String(s) => s,
        other => {
            *broken |= !other.is_null();
            ""
        }
    }
}

/// The fragments of one reply, in first-seen order.
pub(super) struct Calls {
    slots: Vec<Slot>,
    /// Bytes of ids, names and arguments kept so far.
    bytes: usize,
    overflowed: bool,
    /// The last fragment was dropped past the cap: its index-less
    /// continuations are dropped too.
    dropping: bool,
    /// A `finish_reason` other than `tool_calls` or `stop` was seen: the
    /// reply was cut off or filtered, so no call is released.
    cut: bool,
    external: Qualified,
    families: FamilySet,
}

impl Calls {
    pub(super) fn new(external: Qualified, families: FamilySet) -> Self {
        Self {
            slots: Vec::new(),
            bytes: 0,
            overflowed: false,
            dropping: false,
            cut: false,
            external,
            families,
        }
    }

    /// Folds the `tool_calls` fragments of one chunk into the slots. Past the
    /// byte cap it returns the error that ends the stream.
    pub(super) fn collect(&mut self, value: &Value) -> Option<StreamEvent> {
        if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
            self.cut |= !matches!(reason, "tool_calls" | "stop");
        }
        let list = value["choices"][0]["delta"]["tool_calls"].as_array()?;
        for entry in list.iter().filter(|e| e.is_object()) {
            let index = entry["index"].as_u64();
            let function = &entry["function"];
            let mut broken = false;
            let (id, name, args) = (
                text(&entry["id"], &mut broken),
                text(&function["name"], &mut broken),
                text(&function["arguments"], &mut broken),
            );
            self.bytes += id.len() + name.len() + args.len();
            if self.bytes > SERVER_LINE_CAP {
                return Some(too_long());
            }
            let found = index.and_then(|i| self.slots.iter().position(|s| s.index == Some(i)));
            let at = match found {
                Some(at) => at,
                None if index.is_some() || !(id.is_empty() && name.is_empty()) => {
                    if self.slots.len() == MAX_CALLS {
                        self.overflowed = true;
                        self.dropping = true;
                        continue;
                    }
                    self.slots.push(Slot {
                        index,
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                        broken: false,
                    });
                    self.slots.len() - 1
                }
                None if self.dropping => continue,
                None => match self.slots.len().checked_sub(1) {
                    Some(last) => last,
                    None => continue,
                },
            };
            self.dropping = false;
            let slot = &mut self.slots[at];
            if slot.id.is_empty() {
                slot.id = id.to_owned();
            }
            slot.name.push_str(name);
            slot.arguments.push_str(args);
            slot.broken |= broken;
        }
        None
    }

    /// The assembled calls, validated like an Ollama call, then one refusal
    /// for any slot past [`MAX_CALLS`].
    pub(super) fn release(&mut self) -> Vec<ToolRequest> {
        let mut calls: Vec<ToolRequest> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(|s| {
                let arguments = match (s.broken, s.arguments.is_empty()) {
                    (true, _) => Value::Null,
                    (false, true) => "{}".into(),
                    (false, false) => s.arguments.into(),
                };
                let mut entry = json!({"function": {
                    "name": s.name, "arguments": arguments, "index": s.index,
                }});
                if !s.id.is_empty() {
                    entry["id"] = s.id.into();
                }
                tool_call(&entry, &self.external, self.families)
            })
            .collect();
        if std::mem::take(&mut self.overflowed) {
            calls.push(refused(CallError::TooMany));
        }
        calls
    }
}

/// [`SseParser`] that also collects the tool calls of the reply. Text and the
/// end of the stream decode exactly as before; the calls ride beside the
/// events and are released only after a clean end.
pub struct OpenAiToolDecoder {
    parser: SseParser,
    clean: bool,
}

impl OpenAiToolDecoder {
    pub fn server() -> Self {
        Self::server_with(Qualified::default())
    }

    /// A server decoder that also accepts the qualified names of `external`.
    pub fn server_with(external: Qualified) -> Self {
        let mut parser = SseParser::server();
        parser.calls = Some(Calls::new(external, FamilySet::LEGACY));
        Self {
            parser,
            clean: false,
        }
    }

    /// This decoder with the built-ins of `families` callable.
    pub fn with_families(mut self, families: FamilySet) -> Self {
        if let Some(calls) = self.parser.calls.as_mut() {
            calls.families = families;
        }
        self
    }

    /// The calls of the reply after a clean end; the decoder forgets them.
    pub fn take_calls(&mut self) -> Vec<ToolRequest> {
        match self.parser.calls.as_mut() {
            Some(calls) if self.clean && !calls.cut => calls.release(),
            _ => Vec::new(),
        }
    }

    fn scan(&mut self, events: &[StreamEvent]) {
        for event in events {
            match event {
                StreamEvent::Done(_) => self.clean = true,
                StreamEvent::Error(_) => self.clean = false,
                _ => {}
            }
        }
    }
}

impl ToolCalls for OpenAiToolDecoder {
    fn take_calls(&mut self) -> Vec<ToolRequest> {
        Self::take_calls(self)
    }
}

impl StreamDecoder for OpenAiToolDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        let events = self.parser.feed(bytes);
        self.scan(&events);
        events
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let events = self.parser.finish();
        self.scan(&events);
        events
    }
}

//! The OpenAI-style tool decoder: `delta.tool_calls` fragments of an SSE
//! stream, assembled per `index` and released through the same `tool_call()`
//! the Ollama decoder uses.

use serde_json::{Value, json};

use super::{SseParser, StreamDecoder, StreamEvent, ToolCalls, ToolRequest, tool_call};
use crate::core::ai::tools::{registry::FamilySet, servers::Qualified};

/// One call being assembled.
struct Slot {
    index: Option<u64>,
    id: String,
    name: String,
    arguments: String,
}

/// The fragments of one reply, in first-seen order.
pub(super) struct Calls {
    slots: Vec<Slot>,
    external: Qualified,
    families: FamilySet,
}

impl Calls {
    pub(super) fn new(external: Qualified, families: FamilySet) -> Self {
        Self {
            slots: Vec::new(),
            external,
            families,
        }
    }

    /// Folds the `tool_calls` fragments of one chunk into the slots.
    pub(super) fn collect(&mut self, value: &Value) {
        let Some(list) = value["choices"][0]["delta"]["tool_calls"].as_array() else {
            return;
        };
        for entry in list.iter().filter(|e| e.is_object()) {
            let index = entry["index"].as_u64();
            let function = &entry["function"];
            let id = entry["id"].as_str().unwrap_or("");
            let name = function["name"].as_str().unwrap_or("");
            let found = index.and_then(|i| self.slots.iter().position(|s| s.index == Some(i)));
            let at = match found {
                Some(at) => at,
                None if index.is_some() || !(id.is_empty() && name.is_empty()) => {
                    self.slots.push(Slot {
                        index,
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                    self.slots.len() - 1
                }
                None => match self.slots.len().checked_sub(1) {
                    Some(last) => last,
                    None => continue,
                },
            };
            let slot = &mut self.slots[at];
            if slot.id.is_empty() {
                slot.id = id.to_owned();
            }
            slot.name.push_str(name);
            slot.arguments
                .push_str(function["arguments"].as_str().unwrap_or(""));
        }
    }

    /// The assembled calls, validated like an Ollama call.
    pub(super) fn release(&mut self) -> Vec<ToolRequest> {
        std::mem::take(&mut self.slots)
            .into_iter()
            .map(|s| {
                let mut entry = json!({"function": {
                    "name": s.name, "arguments": s.arguments, "index": s.index,
                }});
                if !s.id.is_empty() {
                    entry["id"] = s.id.into();
                }
                tool_call(&entry, &self.external, self.families)
            })
            .collect()
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
            Some(calls) if self.clean => calls.release(),
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

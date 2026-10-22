//! The table behind the built-in tools: one `ToolDef` per tool, one `ArgDef`
//! per argument. The schema and the parse both read it, so they cannot
//! drift apart. Pure: no IO.

use serde_json::{Map, Value, json};

use super::{Access, ToolError};

/// The group a tool (or a gated argument) belongs to. A family is offered
/// only while its toggle is on; `Read` and `Pane` are the 0.18.0 tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Read,
    Search,
    Write,
    Exec,
    Git,
    GitWrite,
    Pane,
}

/// The type of one argument and the limits the parse enforces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgKind {
    /// A string of at most `max` bytes.
    Str {
        max: usize,
    },
    /// An integer, clamped into `min..=max` and never refused for its size.
    Int {
        min: i64,
        max: i64,
    },
    Bool,
    /// At most `items` strings of at most `each` bytes.
    StrList {
        items: usize,
        each: usize,
    },
}

/// One argument of a tool. A `gate` makes it exist only while that family
/// is on: with the family off it is an unknown key.
#[derive(Clone, Copy, Debug)]
pub struct ArgDef {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
    pub gate: Option<Family>,
}

/// One built-in tool: what is offered and how its arguments are read.
#[derive(Clone, Copy, Debug)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub access: Access,
    pub family: Family,
    pub args: &'static [ArgDef],
}

/// A validated argument.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ArgValue {
    Str(String),
    Int(i64),
    Bool(bool),
    List(Vec<String>),
}

/// The arguments of a call that passed [`parse_args`], in `ArgDef` order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parsed(Vec<(&'static str, ArgValue)>);

impl Parsed {
    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.0
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, v)| v)
    }
}

/// The families of the 0.18.0 tools, the only ones offered until a toggle
/// exists.
pub fn legacy(family: Family) -> bool {
    matches!(family, Family::Read | Family::Pane)
}

/// The JSON schema of a tool, from its table: the arguments whose family is
/// on, `additionalProperties: false`.
pub fn schema(def: &ToolDef, on: impl Fn(Family) -> bool) -> Value {
    let live = def.args.iter().filter(|arg| arg.gate.is_none_or(&on));
    let mut properties = Map::new();
    let mut required = Vec::new();
    for arg in live {
        properties.insert(arg.name.to_owned(), kind_schema(&arg.kind));
        if arg.required {
            required.push(Value::from(arg.name));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn kind_schema(kind: &ArgKind) -> Value {
    match kind {
        ArgKind::Str { .. } => json!({ "type": "string" }),
        ArgKind::Int { min, max } => json!({ "type": "integer", "minimum": min, "maximum": max }),
        ArgKind::Bool => json!({ "type": "boolean" }),
        ArgKind::StrList { items, .. } => {
            json!({ "type": "array", "items": { "type": "string" }, "maxItems": items })
        }
    }
}

/// Reads `args` against `defs`, in the 0.18.0 order: not an object, then per
/// argument (missing, wrong type), then an extra key, then too long. `on`
/// says whether a family is offered; a gated argument of a family that is off
/// is an extra key.
pub fn parse_args(
    defs: &[ArgDef],
    args: &Value,
    on: impl Fn(Family) -> bool,
) -> Result<Parsed, ToolError> {
    let object = args.as_object().ok_or(ToolError::NotAnObject)?;
    let live: Vec<&ArgDef> = defs.iter().filter(|def| def.gate.is_none_or(&on)).collect();
    let mut parsed = Vec::new();
    for def in &live {
        match object.get(def.name) {
            None if def.required => return Err(ToolError::MissingArgument),
            None => {}
            Some(value) => parsed.push((def.name, typed(&def.kind, value)?)),
        }
    }
    if object
        .keys()
        .any(|key| live.iter().all(|def| def.name != key))
    {
        return Err(ToolError::ExtraArgument);
    }
    for (def, (_, value)) in live
        .iter()
        .filter(|d| object.contains_key(d.name))
        .zip(&parsed)
    {
        if too_long(&def.kind, value) {
            return Err(ToolError::TooLong);
        }
    }
    Ok(Parsed(parsed))
}

/// The value as its kind, or `WrongType`. Ints are clamped here.
fn typed(kind: &ArgKind, value: &Value) -> Result<ArgValue, ToolError> {
    let wrong = ToolError::WrongType;
    Ok(match kind {
        ArgKind::Str { .. } => ArgValue::Str(value.as_str().ok_or(wrong)?.to_owned()),
        ArgKind::Int { min, max } => {
            // A u64 past i64::MAX is only a big number: it clamps to `max`.
            let n = match (value.as_i64(), value.as_u64()) {
                (Some(n), _) => n,
                (None, Some(_)) => i64::MAX,
                (None, None) => return Err(wrong),
            };
            ArgValue::Int(n.clamp(*min, *max))
        }
        ArgKind::Bool => ArgValue::Bool(value.as_bool().ok_or(wrong)?),
        ArgKind::StrList { .. } => {
            let items = value.as_array().ok_or(wrong)?;
            let texts: Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect();
            ArgValue::List(texts.ok_or(wrong)?)
        }
    })
}

fn too_long(kind: &ArgKind, value: &ArgValue) -> bool {
    match (kind, value) {
        (ArgKind::Str { max }, ArgValue::Str(text)) => text.len() > *max,
        (ArgKind::StrList { items, each }, ArgValue::List(list)) => {
            list.len() > *items || list.iter().any(|text| text.len() > *each)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ARGS: &[ArgDef] = &[
        ArgDef {
            name: "path",
            kind: ArgKind::Str { max: 8 },
            required: true,
            gate: None,
        },
        ArgDef {
            name: "timeout_secs",
            kind: ArgKind::Int { min: 1, max: 600 },
            required: false,
            gate: Some(Family::Exec),
        },
        ArgDef {
            name: "all",
            kind: ArgKind::Bool,
            required: false,
            gate: None,
        },
        ArgDef {
            name: "paths",
            kind: ArgKind::StrList { items: 2, each: 3 },
            required: false,
            gate: None,
        },
    ];

    fn parse(args: Value, exec: bool) -> Result<Parsed, ToolError> {
        parse_args(ARGS, &args, |family| family != Family::Exec || exec)
    }

    // Spec (agent-tools, argument parse order): unknown -> not an object ->
    // per argument (missing, wrong type) -> extra key -> too long.
    #[test]
    fn errors_come_in_the_fixed_order() {
        assert_eq!(parse(json!([1]), true), Err(ToolError::NotAnObject));
        assert_eq!(parse(json!({}), true), Err(ToolError::MissingArgument));
        assert_eq!(
            parse(json!({"path": 1, "extra": 1}), true),
            Err(ToolError::WrongType)
        );
        assert_eq!(
            parse(json!({"path": "a", "extra": 1}), true),
            Err(ToolError::ExtraArgument)
        );
        // Too long is last: an extra key wins over an oversized value.
        assert_eq!(
            parse(json!({"path": "a".repeat(9), "extra": 1}), true),
            Err(ToolError::ExtraArgument)
        );
        assert_eq!(
            parse(json!({"path": "a".repeat(9)}), true),
            Err(ToolError::TooLong)
        );
    }

    #[test]
    fn valid_arguments_are_returned_by_name() {
        let got = parse(
            json!({"path": "abc", "all": true, "paths": ["a", "bc"]}),
            true,
        )
        .unwrap();
        assert_eq!(got.get("path"), Some(&ArgValue::Str("abc".into())));
        assert_eq!(got.get("all"), Some(&ArgValue::Bool(true)));
        assert_eq!(
            got.get("paths"),
            Some(&ArgValue::List(vec!["a".into(), "bc".into()]))
        );
        assert_eq!(got.get("timeout_secs"), None);
    }

    #[test]
    fn list_and_bool_arguments_are_type_and_size_checked() {
        let bad = |args: Value| parse(args, true);
        assert_eq!(
            bad(json!({"path": "a", "all": "yes"})),
            Err(ToolError::WrongType)
        );
        assert_eq!(
            bad(json!({"path": "a", "paths": "a"})),
            Err(ToolError::WrongType)
        );
        assert_eq!(
            bad(json!({"path": "a", "paths": [1]})),
            Err(ToolError::WrongType)
        );
        assert_eq!(
            bad(json!({"path": "a", "paths": ["a", "b", "c"]})),
            Err(ToolError::TooLong)
        );
        assert_eq!(
            bad(json!({"path": "a", "paths": ["abcd"]})),
            Err(ToolError::TooLong)
        );
    }

    // Spec: an integer outside its range is clamped, never refused.
    #[test]
    fn integers_are_clamped_to_their_range() {
        for (given, want) in [(900, 600), (0, 1), (-5, 1), (30, 30)] {
            let got = parse(json!({"path": "a", "timeout_secs": given}), true).unwrap();
            assert_eq!(
                got.get("timeout_secs"),
                Some(&ArgValue::Int(want)),
                "{given}"
            );
        }
        assert_eq!(
            parse(json!({"path": "a", "timeout_secs": 1.5}), true),
            Err(ToolError::WrongType)
        );
    }

    // "Clamped, never refused for size": a u64 past i64::MAX is just big.
    #[test]
    fn integers_past_i64_are_clamped_and_floats_are_refused() {
        let args = |n: Value| json!({"path": "a", "timeout_secs": n});
        let got = parse(args(json!(u64::MAX)), true).unwrap();
        assert_eq!(got.get("timeout_secs"), Some(&ArgValue::Int(600)));
        let got = parse(args(json!(i64::MIN)), true).unwrap();
        assert_eq!(got.get("timeout_secs"), Some(&ArgValue::Int(1)));
        assert_eq!(parse(args(json!(5.0)), true), Err(ToolError::WrongType));
    }

    // Spec: a gated argument exists only while its family is on.
    #[test]
    fn a_gated_argument_is_accepted_only_when_its_family_is_on() {
        let args = json!({"path": "a", "timeout_secs": 5});
        assert!(parse(args.clone(), true).is_ok());
        assert_eq!(parse(args, false), Err(ToolError::ExtraArgument));
    }

    fn def() -> ToolDef {
        ToolDef {
            name: "demo",
            description: "d",
            access: Access::Read,
            family: Family::Read,
            args: ARGS,
        }
    }

    // The schema is the table: the same ArgDefs the parse reads.
    #[test]
    fn the_schema_is_built_from_the_table() {
        let on = |family| family != Family::Exec;
        assert_eq!(
            schema(&def(), on),
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "all": {"type": "boolean"},
                    "paths": {"type": "array", "items": {"type": "string"}, "maxItems": 2},
                },
                "required": ["path"],
                "additionalProperties": false,
            })
        );
    }

    #[test]
    fn a_gated_argument_appears_in_the_schema_only_when_its_family_is_on() {
        let with = schema(&def(), |_| true);
        assert_eq!(
            with["properties"]["timeout_secs"],
            json!({"type": "integer", "minimum": 1, "maximum": 600})
        );
        let without = schema(&def(), |family| family != Family::Exec);
        assert!(without["properties"].get("timeout_secs").is_none());
    }
}

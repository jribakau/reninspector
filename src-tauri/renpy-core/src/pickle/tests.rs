use super::opcode::*;
use super::policy::{Globals, Limits, Policy};
use super::read::{live_containers, MAX_DEPTH};
use super::{load, Value};

fn script(data: &[u8]) -> Result<Value, super::PickleError> {
    load(data, &Policy::SCRIPT)
}

fn save(data: &[u8]) -> Result<Value, super::PickleError> {
    load(data, &Policy::SAVE)
}

fn archive(data: &[u8]) -> Result<Value, super::PickleError> {
    load(data, &Policy::ARCHIVE_INDEX)
}

#[test]
fn builds_an_object_without_calling_it() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"renpy.ast\nSay\n");
    p.push(EMPTY_TUPLE);
    p.push(REDUCE);
    p.push(EMPTY_DICT);
    p.push(BINUNICODE);
    p.extend(4u32.to_le_bytes());
    p.extend(b"what");
    p.push(BINUNICODE);
    p.extend(2u32.to_le_bytes());
    p.extend(b"Hi");
    p.push(SETITEM);
    p.push(BUILD);
    p.push(STOP);
    match script(&p).unwrap() {
        Value::Object { class, state, .. } => {
            assert_eq!(class, "renpy.ast.Say");
            let state = state.unwrap();
            match *state {
                Value::Dict(items) => assert!(items.iter().any(|(k, v)| matches!(
                    (k, v),
                    (Value::Str(k), Value::Str(v)) if k == "what" && v == "Hi"
                ))),
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn save_accepts_a_module_that_a_script_refuses() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"os\nsystem\n");
    p.push(EMPTY_TUPLE);
    p.push(REDUCE);
    p.push(STOP);
    assert!(script(&p).is_err());
    match save(&p).unwrap() {
        Value::Object { class, .. } => assert_eq!(class, "os.system"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn save_keeps_items_appended_to_an_unknown_list_subclass() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"game\nMyList\n");
    p.extend([
        EMPTY_TUPLE,
        REDUCE,
        MARK,
        BININT1,
        1,
        BININT1,
        2,
        APPENDS,
        STOP,
    ]);
    assert!(script(&p).is_err());
    match save(&p).unwrap() {
        Value::Object { class, args, .. } => {
            assert_eq!(class, "game.MyList");
            assert_eq!(args, vec![Value::Int(1), Value::Int(2)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn truncated_pickle_is_an_error() {
    let err = script(&[PROTO, 2, BININT]).unwrap_err();
    assert!(err.0.contains("ended early"), "{err}");
}

#[test]
fn stack_limit_is_enforced() {
    let mut p = vec![PROTO, 2];
    for _ in 0..=100_000 {
        p.push(BININT1);
        p.push(1);
    }
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("stack is too deep"), "{err}");
}

#[test]
fn oversized_long_is_rejected() {
    let mut p = vec![PROTO, 2, LONG1, 17];
    p.extend(std::iter::repeat_n(0u8, 17));
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("integer is too big"), "{err}");
}

#[test]
fn sixteen_byte_long_keeps_its_sign() {
    let mut p = vec![PROTO, 2, LONG1, 16];
    p.extend([0xffu8; 16]);
    p.push(STOP);
    assert_eq!(script(&p).unwrap(), Value::Int(-1));
}

#[test]
fn huge_binbytes8_length_is_an_error() {
    let mut p = vec![PROTO, 4, BINBYTES8];
    p.extend((u64::MAX - 1).to_le_bytes());
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(
        err.0.contains("too large") || err.0.contains("ended early"),
        "{err}"
    );
}

#[test]
fn cyclic_back_edge_keeps_the_other_fields() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"renpy.ast\nSay\n");
    p.extend([EMPTY_TUPLE, REDUCE, BINPUT, 1, EMPTY_DICT]);
    p.push(BINUNICODE);
    p.extend(4u32.to_le_bytes());
    p.extend(b"what");
    p.push(BINUNICODE);
    p.extend(2u32.to_le_bytes());
    p.extend(b"Hi");
    p.push(SETITEM);
    p.push(BINUNICODE);
    p.extend(4u32.to_le_bytes());
    p.extend(b"next");
    p.extend([BINGET, 1, SETITEM, BUILD, STOP]);
    match script(&p).unwrap() {
        Value::Object { class, state, .. } => {
            assert_eq!(class, "renpy.ast.Say");
            let state = state.unwrap();
            let Value::Dict(items) = state.as_ref() else {
                panic!("state is not a dict");
            };
            assert!(items.iter().any(|(k, v)| matches!(
                (k, v),
                (Value::Str(k), Value::Str(v)) if k == "what" && v == "Hi"
            )));
            assert!(items
                .iter()
                .any(|(k, v)| matches!((k, v), (Value::Str(k), Value::None) if k == "next")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn cyclic_set_is_cut() {
    let p = vec![
        PROTO, 4, EMPTY_SET, BINPUT, 1, MARK, BINGET, 1, ADDITEMS, STOP,
    ];
    match script(&p).unwrap() {
        Value::Set(items) => assert_eq!(items, vec![Value::None]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn cyclic_list_is_cut_and_dropped() {
    let p = vec![PROTO, 2, EMPTY_LIST, BINPUT, 1, BINGET, 1, APPEND, STOP];
    match script(&p).unwrap() {
        Value::List(items) => assert_eq!(items, vec![Value::None]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn deep_tuples_hit_the_nesting_limit() {
    let mut p = vec![PROTO, 2, NONE_OP];
    p.extend(std::iter::repeat_n(TUPLE1, MAX_DEPTH + 1));
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("too deep"), "{err}");
}

#[test]
fn a_million_nested_tuples_do_not_overflow_the_stack() {
    let mut p = vec![PROTO, 2, NONE_OP];
    p.extend(std::iter::repeat_n(TUPLE1, 1_000_000));
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("too deep"), "{err}");
}

/// `NONE` wrapped in `n` one-element tuples.
fn deep_tuple(n: usize) -> Vec<u8> {
    let mut p = vec![NONE_OP];
    p.extend(std::iter::repeat_n(TUPLE1, n));
    p
}

#[test]
fn a_deep_value_popped_mid_parse_does_not_overflow_the_stack() {
    let mut p = vec![PROTO, 2];
    p.extend(deep_tuple(1_000_000));
    p.extend([POP, NONE_OP, STOP]);
    assert_eq!(script(&p).unwrap(), Value::None);
    assert_eq!(live_containers(), 0);
}

#[test]
fn a_deep_value_dropped_on_an_error_path_does_not_overflow_the_stack() {
    // The odd item count makes DICT fail with the deep value in its item list.
    let mut p = vec![PROTO, 2, MARK];
    p.extend(deep_tuple(1_000_000));
    p.extend([DICT_OP, STOP]);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("odd number"), "{err}");
    assert_eq!(live_containers(), 0);
}

#[test]
fn a_deep_value_replaced_in_the_memo_does_not_overflow_the_stack() {
    let mut p = vec![PROTO, 2];
    p.extend(deep_tuple(1_000_000));
    p.extend([BINPUT, 0, POP, NONE_OP, BINPUT, 0, STOP]);
    assert_eq!(script(&p).unwrap(), Value::None);
    assert_eq!(live_containers(), 0);
}

#[test]
fn a_deep_value_in_an_object_does_not_overflow_the_stack() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"renpy.ast\nSay\n");
    p.extend(deep_tuple(1_000_000));
    p.extend([TUPLE1, REDUCE, POP, NONE_OP, STOP]);
    assert_eq!(script(&p).unwrap(), Value::None);
    assert_eq!(live_containers(), 0);
}

#[test]
fn cyclic_structures_are_freed() {
    // A list that contains itself.
    let list = vec![PROTO, 2, EMPTY_LIST, BINPUT, 1, BINGET, 1, APPEND, STOP];
    script(&list).unwrap();
    assert_eq!(live_containers(), 0);

    // A dict that holds a list that holds the dict.
    let dict = vec![
        PROTO, 2, EMPTY_DICT, BINPUT, 1, BININT1, 7, EMPTY_LIST, BINPUT, 2, BINGET, 1, APPEND,
        SETITEM, STOP,
    ];
    script(&dict).unwrap();
    assert_eq!(live_containers(), 0);

    // The same cycle in an archive index is an error, and is still freed.
    assert!(archive(&list).is_err());
    assert_eq!(live_containers(), 0);
}

#[test]
fn shared_structure_stops_at_the_value_budget() {
    let policy = Policy {
        globals: Globals::Allowlist,
        limits: Limits {
            max_objects: 10_000,
            max_bytes: 1_000_000,
            max_values: 1_000,
            max_output_bytes: 1_000_000,
            max_input: None,
        },
    };
    let mut lists = vec![PROTO, 2, EMPTY_LIST, BINPUT, 0];
    for k in 1u8..=40 {
        lists.extend([
            EMPTY_LIST,
            BINPUT,
            k,
            MARK,
            BINGET,
            k - 1,
            BINGET,
            k - 1,
            APPENDS,
        ]);
    }
    lists.push(STOP);
    let err = load(&lists, &policy).unwrap_err();
    assert!(err.0.contains("too many values"), "{err}");

    let mut tuples = vec![PROTO, 2, NONE_OP, BINPUT, 0];
    for k in 1u8..=40 {
        tuples.extend([BINGET, k - 1, BINGET, k - 1, TUPLE2, BINPUT, k]);
    }
    tuples.push(STOP);
    let err = load(&tuples, &policy).unwrap_err();
    assert!(err.0.contains("too many values"), "{err}");
}

#[test]
fn rejects_a_module_outside_the_allow_list() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"os\nsystem\n");
    p.push(EMPTY_TUPLE);
    p.push(REDUCE);
    p.push(STOP);
    let err = script(&p).unwrap_err();
    assert!(err.0.contains("not allowed"), "{err}");
}

#[test]
fn codecs_encode_is_the_only_archive_global() {
    let mut p = vec![PROTO, 2, GLOBAL];
    p.extend(b"_codecs\nencode\n");
    p.push(BINUNICODE);
    p.extend_from_slice(&2u32.to_le_bytes());
    p.extend(b"AB");
    p.push(BINUNICODE);
    p.extend_from_slice(&6u32.to_le_bytes());
    p.extend(b"latin1");
    p.push(TUPLE2);
    p.push(REDUCE);
    p.push(STOP);
    assert_eq!(archive(&p).unwrap(), Value::Bytes(b"AB".to_vec()));

    let bad = b"cos\nsystem\n.".to_vec();
    let err = archive(&bad).unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");
}

#[test]
fn archive_rejects_a_float_and_a_truncated_stream() {
    assert!(archive(&[PROTO, 2]).is_err());
    assert!(archive(&[PROTO, 2, FLOAT, b'.']).is_err());
    assert!(archive(&[]).is_err());
}

#[test]
fn archive_cycle_is_an_error() {
    let p = vec![PROTO, 2, EMPTY_LIST, BINPUT, 1, BINGET, 1, APPEND, STOP];
    let err = archive(&p).unwrap_err();
    assert!(err.0.contains("cyclic"), "{err}");
}

#[test]
fn raw_unicode_decodes_wide_escapes_and_latin1() {
    let mut wide = vec![UNICODE];
    wide.extend(br"\U0001F600");
    wide.extend([b'\n', STOP]);
    assert_eq!(script(&wide).unwrap(), Value::Str("\u{1F600}".into()));

    let mut pair = vec![UNICODE];
    pair.extend(br"\uD83D\uDE00");
    pair.extend([b'\n', STOP]);
    assert_eq!(script(&pair).unwrap(), Value::Str("\u{1F600}".into()));

    let latin = vec![UNICODE, 0xE9, b'\n', STOP];
    assert_eq!(script(&latin).unwrap(), Value::Str("\u{00E9}".into()));
}

#[test]
fn protocol0_string_rejects_trailing_bytes() {
    let err = script(b"S'ab'c\n.").unwrap_err();
    assert!(err.0.contains("trailing"), "{err}");
}

#[test]
fn save_rejects_an_oversized_input() {
    let err = save(&vec![0; 32 * 1024 * 1024 + 1]).unwrap_err();
    assert!(err.0.contains("too large"), "{err}");
}

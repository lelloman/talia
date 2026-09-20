# P2 engine values

TALIA-24 extends the frozen P0 JSON boundary. Runtime values include `undefined`,
`null`, booleans, strings, IEEE-754 numbers (including NaN, ±Infinity and -0),
arrays and plain string-keyed objects. Functions, accessors, symbols, BigInt,
cycles and native objects are rejected. Sparse arrays normalize holes to undefined.

Wire/storage envelopes are `{"version":1,"value":NODE}`. Every node is tagged:
`["undefined"]`, `["null"]`, `["boolean",true]`, `["string","text"]`,
`["number",42]`, `["number","NaN"]` (also Infinity, -Infinity, -0),
`["array",[NODE,...]]`, or `["object",[["key",NODE],...]]`.
User objects always use object nodes, so tag-shaped data is not interpreted.
Duplicate object keys and unknown versions/tags fail validation. Limits are
128 KiB UTF-8, depth 48 and 16,000 nodes. Object keys are canonically sorted in JS;
equality ignores insertion order, treats NaN as equal to itself, and distinguishes
-0 from 0 and absent properties from undefined. Exact identifiers use strings.

Initial declared schemas are top-level types or `any`; nested data remains bounded
and codec-validated. Changing a schema requires all migrated values/state to validate.
An absent measurement is represented by metadata, never by an undefined sentinel.
Quality, timestamp, evaluation status and error remain separate from the value.

The codec is portable JavaScript shared by browser and embedded QuickJS; Rust
validates the tagged representation without lossy conversion through JSON numbers.
Renderer defaults must visibly distinguish exceptional values; formatting overrides
belong to widgets. Live heaps and credentials are never persistent state.

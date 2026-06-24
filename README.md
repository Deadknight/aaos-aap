# v20d protobuf Enum trait import fix

`protobuf-codegen` generated enums implement `protobuf::Enum`; methods like
`from_i32()` and `value()` are trait methods, so the trait must be in scope.

This patch updates:

```rust
use protobuf::{Enum, Message};
```

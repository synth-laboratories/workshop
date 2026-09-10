# Log runtime

App-local structured observations correlated to operations and failures.

Does **not** own failure identity or state transitions. `eprintln!` is forbidden outside `emergency_sink.rs`.

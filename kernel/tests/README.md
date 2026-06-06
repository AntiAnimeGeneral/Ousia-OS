Host integration tests in this directory now protect Ousia-native kernel behavior contracts.

Old CNode path, Untyped retype, and seL4-style executor tests are reference evidence only. New tests should exercise the current handle/object/process/syscall boundary and assert owner state after both success and failure paths.

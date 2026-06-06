Host integration tests in this directory now protect Ousia-native kernel behavior contracts.

Old CNode path, Untyped retype, ObjectTable, Endpoint/Reply, and seL4-style executor tests are no longer active compatibility targets. New tests should exercise the current handle/object/process/syscall boundary and assert owner state after both success and failure paths.

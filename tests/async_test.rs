//! Asynchronous contracts using the test target's current-thread executor.

/// Runs each test on its own runtime and propagates runtime-creation failures.
macro_rules! async_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() -> Result<(), std::io::Error> {
            crate::run_async(Box::pin(async $body))
        }
    };
    ($name:ident -> $output:ty, $body:block) => {
        #[test]
        fn $name() -> $output {
            crate::run_async(Box::pin(async $body))?
        }
    };
}

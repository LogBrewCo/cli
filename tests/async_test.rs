//! Shared current-thread executor for asynchronous test contracts.

/// Runs one boxed asynchronous test without a procedural macro expansion.
pub(crate) fn run_async<T>(future: std::pin::Pin<Box<dyn Future<Output = T>>>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime builds")
        .block_on(future)
}

macro_rules! async_test {
    ($name:ident $(-> $output:ty)?, $body:block) => {
        #[test]
        fn $name() $(-> $output)? {
            crate::run_async(Box::pin(async $body))
        }
    };
}

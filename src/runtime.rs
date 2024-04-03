use futures::Future;
pub use tokio::{task::spawn, time::sleep, time::timeout};

pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(future)
}

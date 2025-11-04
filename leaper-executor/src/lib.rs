use iced::Executor;

pub struct LeaperExecutor(tokio::runtime::Runtime);

impl Executor for LeaperExecutor {
    fn new() -> Result<Self, futures::io::Error>
    where
        Self: Sized,
    {
        Ok(Self(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(10 * 1024 * 1024)
                .build()?,
        ))
    }

    fn spawn(
        &self,
        future: impl Future<Output = ()> + iced::advanced::graphics::futures::MaybeSend + 'static,
    ) {
        <tokio::runtime::Runtime as Executor>::spawn(&self.0, future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        <tokio::runtime::Runtime as Executor>::enter(&self.0, f)
    }
}

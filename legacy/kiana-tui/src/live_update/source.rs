//! Data source abstraction for live updates.

use std::future::Future;
use std::pin::Pin;

/// A source of data that can be polled for updates.
#[async_trait::async_trait]
pub trait DataSource: Send + Sync {
    type Data: Send;

    /// Fetches the latest data from this source.
    async fn fetch(&self) -> anyhow::Result<Self::Data>;

    /// Checks if the data has changed compared to the previous value.
    ///
    /// Default implementation always returns true (assumes data changed).
    fn has_changed(&self, _old: &Self::Data, _new: &Self::Data) -> bool {
        true
    }
}

/// A data source created from an async function or closure.
pub struct FunctionSource<F, Fut, T>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    func: F,
    _phantom: std::marker::PhantomData<fn() -> (Fut, T)>,
}

impl<F, Fut, T> FunctionSource<F, Fut, T>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    /// Creates a new function-based data source.
    pub fn new(func: F) -> Self {
        Self {
            func,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<F, Fut, T> DataSource for FunctionSource<F, Fut, T>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    type Data = T;

    async fn fetch(&self) -> anyhow::Result<Self::Data> {
        (self.func)().await
    }
}

/// A data source that can be compared for changes.
pub struct ComparableSource<F, Fut, T, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    C: Fn(&T, &T) -> bool + Send + Sync + 'static,
    T: Send + 'static,
{
    fetch_fn: F,
    compare_fn: C,
    _phantom: std::marker::PhantomData<fn() -> (Fut, T)>,
}

impl<F, Fut, T, C> ComparableSource<F, Fut, T, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    C: Fn(&T, &T) -> bool + Send + Sync + 'static,
    T: Send + 'static,
{
    /// Creates a new comparable data source.
    ///
    /// The compare function should return true if the data has changed.
    pub fn new(fetch_fn: F, compare_fn: C) -> Self {
        Self {
            fetch_fn,
            compare_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<F, Fut, T, C> DataSource for ComparableSource<F, Fut, T, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    C: Fn(&T, &T) -> bool + Send + Sync + 'static,
    T: Send + 'static,
{
    type Data = T;

    async fn fetch(&self) -> anyhow::Result<Self::Data> {
        (self.fetch_fn)().await
    }

    fn has_changed(&self, old: &Self::Data, new: &Self::Data) -> bool {
        (self.compare_fn)(old, new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_function_source() {
        let source = FunctionSource::new(|| async { Ok(42) });
        let data = source.fetch().await.unwrap();
        assert_eq!(data, 42);
    }

    #[tokio::test]
    async fn test_function_source_error() {
        let source = FunctionSource::new(|| async {
            Err(anyhow::anyhow!("test error")) as anyhow::Result<i32>
        });
        let result = source.fetch().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_comparable_source() {
        let source = ComparableSource::new(|| async { Ok(42) }, |old, new| old != new);

        let data = source.fetch().await.unwrap();
        assert_eq!(data, 42);

        assert!(!source.has_changed(&42, &42));
        assert!(source.has_changed(&42, &43));
    }
}

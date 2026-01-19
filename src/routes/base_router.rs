use axum::Router;

pub trait BaseRouter {
    fn base_path() -> &'static str;

    fn apply_router_middleware<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        router
    }

    fn router_for<S>(&self) -> Router<S>
    where
        S: Clone + Send + Sync + 'static;
}

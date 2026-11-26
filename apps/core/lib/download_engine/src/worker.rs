use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use axlauncher_network::ReqwestNetwork;
use axlauncher_spi::downloads::FileDownloader;
use axlauncher_worker::downloads::{Reply, Route, Transfer};
use axlauncher_worker::{Cancellation, Responder, serve};

use crate::{Direct, DownloadEngine};

const STATS_INTERVAL: Duration = Duration::from_millis(150);
const ROUTE_CACHE_LIMIT: usize = 8;

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_downloads_v1() -> i32 {
    let downloads = Rc::new(Downloads::default());
    serve(move |transfer, reply, cancel| Rc::clone(&downloads).transfer(transfer, reply, cancel))
}

#[derive(Default)]
struct Downloads {
    routes: RefCell<Vec<(Route, ReqwestNetwork)>>,
    last_stats: Cell<Option<Instant>>,
}

impl Downloads {
    async fn transfer(
        self: Rc<Self>,
        transfer: Transfer,
        reply: Responder<Reply>,
        cancel: Cancellation,
    ) -> Result<(), String> {
        if !cancel.is_pending() {
            return Err("Download cancelled.".into());
        }
        let network = self.network(transfer.route())?;
        let direct = Direct(&network);
        let sources = transfer.sources();
        let operation = direct.download(transfer.job(&sources), transfer.digest());
        let result = self.run(operation, reply, cancel).await;
        reply.status(DownloadEngine::shared().stats());
        result
    }

    async fn run(
        &self,
        operation: impl Future<Output = Result<(), String>>,
        reply: Responder<Reply>,
        cancel: Cancellation,
    ) -> Result<(), String> {
        let mut operation = std::pin::pin!(operation);
        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("Download cancelled.".into()),
                result = &mut operation => return result,
                () = tokio::time::sleep(STATS_INTERVAL) => self.sample(reply),
            }
        }
    }

    fn sample(&self, reply: Responder<Reply>) {
        let now = Instant::now();
        if self
            .last_stats
            .get()
            .is_none_or(|last| now.duration_since(last) >= STATS_INTERVAL)
        {
            self.last_stats.set(Some(now));
            reply.status(DownloadEngine::shared().stats());
        }
    }

    fn network(&self, route: &Route) -> Result<ReqwestNetwork, String> {
        let mut routes = self.routes.borrow_mut();
        if let Some((_, network)) = routes.iter().find(|(cached, _)| cached == route) {
            return Ok(network.clone());
        }
        let network = route.network()?;
        if routes.len() == ROUTE_CACHE_LIMIT {
            routes.remove(0);
        }
        routes.push((route.clone(), network.clone()));
        Ok(network)
    }
}

use crate::Result;
use anyhow::anyhow;
use mime::Mime;
use reqwest::{Url, header};
use std::cell::RefCell;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

#[derive(Default)]
pub(crate) struct Crawler {
    targets: Vec<Url>,
    outdir: Option<String>,
    max_depth: Option<usize>,
    max_rate: Option<usize>,
}

impl Crawler {
    pub fn target(&mut self, url: Url) -> &mut Self {
        self.targets.push(url);
        self
    }

    pub fn outdir(&mut self, outdir: String) -> &mut Self {
        self.outdir = Some(outdir);
        self
    }

    pub fn max_depth(&mut self, max_depth: Option<usize>) -> &mut Self {
        self.max_depth = max_depth;
        self
    }

    pub fn max_rate(&mut self, max_rate: Option<usize>) -> &mut Self {
        self.max_rate = max_rate;
        self
    }

    pub async fn run(&mut self) {
        let outdir = self.outdir.as_deref().expect("`outdir` must be set");
        let max_depth = self.max_depth.unwrap_or(usize::MAX);
        let debouncer = self
            .max_rate
            .map(|p| 1000 / p as u64)
            .filter(|&p| p != 0)
            .map(Duration::from_millis)
            .map(Debouncer::new)
            .map(Arc::new);
        let hclient = reqwest::Client::new();

        let mut set = JoinSet::new();
        for url in self.targets.drain(..) {
            let Some(host) = url.host_str() else {
                tracing::error!("missing host for URL: {url}");
                continue;
            };
            let outdir = Path::new(outdir).join(host);
            let worker = Worker {
                url,
                outdir: Arc::from(outdir),
                hclient: hclient.clone(),
                level: max_depth,
                debouncer: debouncer.clone(),
            };
            set.spawn(async move { worker.run().await });
        }
        set.join_all().await;
    }
}

struct Worker {
    url: Url,
    outdir: Arc<Path>,
    hclient: reqwest::Client,
    /// Zero means the bottom level and we should stop crawling.
    level: usize,
    debouncer: Option<Arc<Debouncer>>,
}

impl Worker {
    async fn run(&self) {
        let Ok(Some(new_urls)) = self
            .process()
            .await
            .map_err(|e| tracing::error!("failed to process '{}': {e}", self.url))
        else {
            return;
        };

        let mut set = JoinSet::new();
        for url in new_urls {
            set.spawn(self.spawn_run(url));
        }
        set.join_all().await;
    }

    // See: <https://github.com/rust-lang/rust/issues/134101>
    fn spawn_run(&self, url: Url) -> impl 'static + Send + Future<Output = ()> {
        tracing::debug!("step into (level={}) {url}", self.level);
        let worker = Self {
            url,
            outdir: self.outdir.clone(),
            hclient: self.hclient.clone(),
            level: self.level - 1,
            debouncer: self.debouncer.clone(),
        };
        async move { worker.run().await }
    }

    /// Downloads the given URL and returns the HTML document if found any.
    async fn process(&self) -> Result<Option<Vec<Url>>> {
        if let Some(deouncer) = self.debouncer.as_ref() {
            deouncer.wait().await;
        }
        tracing::debug!("processing {}", self.url);

        let resp = self
            .hclient
            .get(self.url.clone())
            .send()
            .await?
            .error_for_status()?;

        let content_type = resp
            .headers()
            .get(&header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<Mime>().ok())
            .ok_or_else(|| anyhow!("cannot determine content type"))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| anyhow!("failed to read response body: {e}"))?;
        self.save_resp(&self.url, &bytes)
            .await
            .map_err(|e| tracing::error!("failed to save response: {e}"))
            .ok();

        if self.level != 1 && content_type == mime::TEXT_HTML_UTF_8 {
            self.crawl_html(&bytes).map(Some)
        } else {
            Ok(None)
        }
    }

    fn crawl_html(&self, body: &[u8]) -> Result<Vec<Url>> {
        let body =
            str::from_utf8(body).map_err(|_| anyhow!("found invalid UTF-8 HTML response"))?;

        let sink = UrlCollector {
            src: &self.url,
            urls: RefCell::new(Vec::new()),
        };
        let input = html5ever::buffer_queue::BufferQueue::default();
        input.push_back(html5ever::tendril::Tendril::from(body));

        let tok = html5ever::tokenizer::Tokenizer::new(sink, <_>::default());
        let _ = tok.feed(&input);
        tok.end();
        Ok(tok.sink.urls.into_inner())
    }

    /// Writes the response of the given URL to `{outdir}/{url.path}`.
    async fn save_resp(&self, url: &Url, body: &[u8]) -> Result<()> {
        let path = Path::new(url.path());
        let outdir = &self.outdir.join(
            path.parent()
                .and_then(|p| p.strip_prefix("/").ok())
                .unwrap_or("".as_ref()),
        );
        let fullpath = outdir.join(path.file_name().unwrap_or("index".as_ref()));
        tracing::debug!("saving {url} to {}", fullpath.display());

        tokio::fs::create_dir_all(outdir).await?;
        tokio::fs::write(&fullpath, body).await?;

        Ok(())
    }
}

struct Debouncer {
    lock: Mutex<()>,
    throttle: Duration,
}

impl Debouncer {
    const fn new(throttle: Duration) -> Self {
        Self {
            lock: Mutex::const_new(()),
            throttle,
        }
    }

    async fn wait(&self) {
        let _guard = self.lock.lock().await;
        tokio::time::sleep(self.throttle).await;
    }
}

/// Returns all valid URLs from the input document.
struct UrlCollector<'a> {
    src: &'a Url,
    urls: RefCell<Vec<Url>>,
}
mod url_collector {
    use super::*;
    use html5ever::tokenizer::*;

    impl TokenSink for UrlCollector<'_> {
        type Handle = ();

        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<Self::Handle> {
            let TagToken(tag) = token else {
                return TokenSinkResult::Continue;
            };
            for attr in tag
                .attrs
                .iter()
                .filter(|a| ["src", "href"].contains(&&*a.name.local))
            {
                if let Some(new_url) = self.src.join(&attr.value).ok().filter(|u| u != self.src) {
                    self.urls.borrow_mut().push(new_url);
                }
            }
            TokenSinkResult::Continue
        }
    }
}

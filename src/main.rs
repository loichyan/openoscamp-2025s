mod crawler;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, help = "Add URIs to crawl")]
    url: Vec<String>,
    #[arg(long, help = "Set limitation on requests per second")]
    max_rate: Option<usize>,
    #[arg(long, help = "Set limitation on depth to step in")]
    max_depth: Option<usize>,
    #[arg(short = 'd', long, help = "Set where all downloaded files are stored")]
    outdir: String,
}

fn init_logging() {
    tracing_subscriber::fmt()
        .pretty()
        .with_thread_names(true)
        .with_max_level(tracing::Level::DEBUG)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_logging();
    let args = Args::parse();
    let mut crawler = crawler::Crawler::default();
    for url in args.url {
        crawler.target(url.parse()?);
    }
    crawler
        .outdir(args.outdir)
        .max_depth(args.max_depth)
        .max_rate(args.max_rate);
    crawler.run().await;
    Ok(())
}

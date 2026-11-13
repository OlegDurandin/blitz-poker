use pvp_server::config::Config;
use pvp_server::hub::Hub;
use pvp_server::store::Store;
use std::sync::Arc;
use std::time::Duration;

/// `pvp-server healthcheck`: exit 0 if this container's server answers
/// `/health`. For Docker's HEALTHCHECK; the slim image has no curl.
fn healthcheck() -> ! {
    use std::io::{Read, Write};
    let port = std::env::var("PVP_BIND")
        .ok()
        .and_then(|b| b.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()))
        .unwrap_or(8787);
    let ok = (|| -> std::io::Result<bool> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut out = String::new();
        s.read_to_string(&mut out)?;
        Ok(out.starts_with("HTTP/1.1 200") && out.contains("\"ok\":true"))
    })()
    .unwrap_or(false);
    std::process::exit(if ok { 0 } else { 1 });
}

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        healthcheck();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pvp_server=info".into()),
        )
        .init();
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    if config.dev_auth {
        tracing::warn!(
            "PVP_DEV_AUTH is on: unsigned dev logins are accepted on {}",
            config.bind
        );
    }
    let mut store = Store::open(&config.db_path).unwrap_or_else(|e| {
        eprintln!("cannot open database {}: {e}", config.db_path);
        std::process::exit(2);
    });
    store.set_rewards(
        pvp_server::progress::Rewards::from_env().unwrap_or_else(|e| {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }),
    );
    let bind = config.bind;
    let hub = Arc::new(Hub::new(config, store));

    let ticker = hub.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut n = 0u64;
        loop {
            tick.tick().await;
            ticker.tick();
            n += 1;
            if n.is_multiple_of(4 * 3600) {
                ticker.prune_store();
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|e| {
            eprintln!("cannot listen on {bind}: {e}");
            std::process::exit(2);
        });
    tracing::info!("listening on {bind}");
    axum::serve(listener, pvp_server::server::router(hub))
        .with_graceful_shutdown(async {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("signal handler");
            #[cfg(unix)]
            tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
            #[cfg(not(unix))]
            let _ = ctrl_c.await;
            tracing::info!("shutting down; rooms are already saved");
        })
        .await
        .expect("server");
}

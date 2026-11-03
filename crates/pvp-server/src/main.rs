use pvp_server::config::Config;
use pvp_server::hub::Hub;
use pvp_server::store::Store;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
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
    let store = Store::open(&config.db_path).unwrap_or_else(|e| {
        eprintln!("cannot open database {}: {e}", config.db_path);
        std::process::exit(2);
    });
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

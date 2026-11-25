#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    path::PathBuf,
    time::Duration,
};

use shortcircuit::{
    AdminConfig, Analytics, Auth, BootstrapKind, PublicHosts, admin_router_with_analytics,
    create_snapshot, data_lock, open_database, public_router_with_analytics, restore_snapshot,
};
use tokio::{net::TcpListener, sync::watch, time};
use tracing_subscriber::EnvFilter;

type AppError = Box<dyn Error + Send + Sync>;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

fn bind_address(variable: &str, default: &str) -> Result<SocketAddr, AppError> {
    let address = setting(variable, default)?;
    address
        .parse()
        .map_err(|_| io::Error::other(format!("invalid {variable}, expected IP:port")).into())
}

fn setting(variable: &str, default: &str) -> Result<String, AppError> {
    match env::var(variable) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => Err(error.into()),
    }
}

fn database_path() -> PathBuf {
    env::var_os("DATA_DIR")
        .map_or_else(|| PathBuf::from("data"), PathBuf::from)
        .join("shortcircuit.db")
}

async fn issue_bootstrap(kind: BootstrapKind) -> Result<(), AppError> {
    let database = database_path();
    let _lock = data_lock(&database, false)?;
    let pool = open_database(&database).await?;
    let token = Auth::new(pool.clone()).issue_bootstrap(kind).await?;
    pool.close().await;
    println!("{token}");
    Ok(())
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

async fn wait_for_stop(mut receiver: watch::Receiver<bool>) {
    let _ = receiver.wait_for(|stopped| *stopped).await;
}

async fn signal_to_stop() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

async fn serve() -> Result<(), AppError> {
    let public_addr = bind_address("PUBLIC_BIND", "127.0.0.1:8080")?;
    let admin_addr = bind_address("ADMIN_BIND", "127.0.0.1:8081")?;
    let hosts = PublicHosts::parse(&setting("PUBLIC_HOSTS", "localhost,127.0.0.1,[::1]")?)
        .map_err(io::Error::other)?;
    let config = AdminConfig::new(
        &setting("ADMIN_ORIGIN", "http://localhost:8081")?,
        public_addr.port(),
    )
    .map_err(io::Error::other)?;
    if env::var_os("ADMIN_TOKEN_FILE").is_some() {
        return Err(io::Error::other(
            "Remove ADMIN_TOKEN_FILE. Use setup-token and create scoped API keys.",
        )
        .into());
    }
    let database = database_path();
    let _lock = data_lock(&database, false)?;
    let pool = open_database(&database).await?;

    let public_socket = TcpListener::bind(public_addr).await?;
    let admin_socket = TcpListener::bind(admin_addr).await?;
    tracing::info!(
        public = %public_socket.local_addr()?,
        admin = %admin_socket.local_addr()?,
        version = env!("CARGO_PKG_VERSION"),
        "ShortCircuit ready"
    );

    let (analytics, mut writer) = Analytics::start(pool.clone()).await?;
    let (stop, receiver) = watch::channel(false);
    let public = axum::serve(
        public_socket,
        public_router_with_analytics(pool.clone(), hosts.clone(), analytics.clone()),
    )
    .with_graceful_shutdown(wait_for_stop(receiver.clone()));
    let admin = axum::serve(
        admin_socket,
        admin_router_with_analytics(pool.clone(), hosts, config, analytics)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_for_stop(receiver));
    let stop_deadline = async move {
        if signal_to_stop().await.is_err() {
            tracing::warn!("could not listen for shutdown signals");
        }
        stop.send_replace(true);
        time::sleep(SHUTDOWN_GRACE).await;
    };
    let result = tokio::select! {
        result = async { tokio::try_join!(async { public.await }, async { admin.await }) } => result.map(|_| ()),
        () = stop_deadline => {
            tracing::warn!("HTTP shutdown deadline reached");
            Ok(())
        }
    };
    match time::timeout(Duration::from_secs(2), &mut writer).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => tracing::warn!("analytics writer stopped unexpectedly"),
        Err(_) => {
            writer.abort();
            tracing::warn!("analytics flush deadline reached");
        }
    }
    if time::timeout(Duration::from_secs(2), pool.close())
        .await
        .is_err()
    {
        tracing::warn!("database pool shutdown deadline reached");
    }
    result?;
    Ok(())
}

async fn backup() -> Result<(), AppError> {
    let database = database_path();
    let _lock = data_lock(&database, false)?;
    if !database.is_file() {
        return Err(io::Error::other("no ShortCircuit database exists").into());
    }
    let directory = database
        .parent()
        .ok_or_else(|| io::Error::other("data path has no parent"))?;
    let pool = open_database(&database).await?;
    let result = create_snapshot(&pool, &directory.join("backups")).await;
    pool.close().await;
    println!("{}", result?.display());
    Ok(())
}

async fn restore(source: PathBuf) -> Result<(), AppError> {
    let result = restore_snapshot(&database_path(), &source).await?;
    println!(
        "Restored database schema {} from snapshot created at UTC Unix timestamp {}",
        result.schema_version, result.created_at
    );
    if let Some(path) = result.rollback {
        if result.rollback_is_snapshot {
            println!("Rollback snapshot: {}", path.display());
        } else {
            println!("Saved previous database file: {}", path.display());
        }
    }
    Ok(())
}

fn healthcheck() -> Result<(), AppError> {
    let mut address = bind_address("ADMIN_BIND", "127.0.0.1:8081")?;
    if address.ip().is_unspecified() {
        let new_address = if address.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        };
        address.set_ip(new_address);
    }
    let mut stream = TcpStream::connect_timeout(&address, PROBE_TIMEOUT)?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;
    stream.write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut status = [0; 13];
    stream.read_exact(&mut status)?;
    if &status != b"HTTP/1.1 200 " {
        return Err(io::Error::other("admin listener is not ready").into());
    }
    Ok(())
}

fn main() -> Result<(), AppError> {
    let mut arguments = env::args_os().skip(1);
    let command = arguments.next();
    let target = arguments.next();
    let confirmation = arguments.next();
    let extra = arguments.next();
    match (command, target, confirmation, extra) {
        (Some(command), None, None, None) if command == "healthcheck" => healthcheck(),
        (Some(command), None, None, None) if command == "setup-token" => {
            runtime()?.block_on(issue_bootstrap(BootstrapKind::Setup))
        }
        (Some(command), None, None, None) if command == "recovery-token" => {
            runtime()?.block_on(issue_bootstrap(BootstrapKind::Recovery))
        }
        (Some(command), None, None, None) if command == "--version" => {
            println!("shortcircuit {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        (Some(command), None, None, None) if command == "backup" => {
            runtime()?.block_on(backup())
        }
        (Some(command), Some(source), Some(confirm), None)
            if command == "restore" && confirm == "--confirm" =>
        {
            runtime()?.block_on(restore(PathBuf::from(source)))
        }
        (None, None, None, None) => {
            let level = setting("LOG_LEVEL", "info")?;
            let filter =
                EnvFilter::try_new(level).map_err(|_| io::Error::other("invalid LOG_LEVEL"))?;
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
            runtime()?.block_on(serve())
        }
        _ => Err(io::Error::other(
            "usage: shortcircuit [healthcheck|setup-token|recovery-token|backup|restore FILE --confirm|--version]",
        )
        .into()),
    }
}

//! `smhubd`: the daemon of `smhub-services`. Started by OpenRC as `/opt/bin/smhub-services`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use smhub_api::state::{Config, Hub};
use smhub_store::Store;
use smhub_sys::Sys;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Args {
    root: PathBuf,
    bin_dir: Option<PathBuf>,
    listen: String,
    socket: Option<PathBuf>,
    db: Option<PathBuf>,
    upload_dir: Option<PathBuf>,
    services: bool,
    /// Seconds to watch ESPHome's API, then exit.
    esphome: Option<u64>,
    /// Commands to send while watching: `light:<name>:<r>,<g>,<b>[:<effect>]`, `light:<name>:off`,
    /// `switch:<name>:on|off`, `service:<name>:<text>`.
    esphome_send: Vec<String>,
}

const HELP: &str = "smhubd - the smhub-services daemon

Usage: smhubd [options]
  --root DIR        resolve every OS path under DIR (default /)
  --bin-dir DIR     look for commands in DIR first (tests)
  --listen ADDR     TCP address (default 127.0.0.1:8000)
  --socket PATH     local socket, or 'none' (default /run/smhub-backend.sock)
  --db PATH         the backend database (default /opt/smhub-services/config/backend.db)
  --upload-dir DIR  where upload pieces go (default /mnt/user/.smhubd)
  --services        print every service's state as the daemon reads it, then exit
  --esphome SECS    connect to ESPHome's API, print its entities and states for SECS, then exit
  --esphome-send C  with --esphome: send C after listing (light:NAME:R,G,B[:EFFECT],
                    light:NAME:off, switch:NAME:on|off, service:NAME:TEXT); may repeat
  -V, --version     print the version
";

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        root: PathBuf::from("/"),
        bin_dir: None,
        listen: "127.0.0.1:8000".into(),
        socket: Some(PathBuf::from("/run/smhub-backend.sock")),
        db: None,
        upload_dir: None,
        services: false,
        esphome: None,
        esphome_send: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--root" => a.root = PathBuf::from(val("--root")?),
            "--bin-dir" => a.bin_dir = Some(PathBuf::from(val("--bin-dir")?)),
            "--listen" => a.listen = val("--listen")?,
            "--socket" => {
                let v = val("--socket")?;
                a.socket = if v == "none" { None } else { Some(PathBuf::from(v)) };
            }
            "--db" => a.db = Some(PathBuf::from(val("--db")?)),
            "--upload-dir" => a.upload_dir = Some(PathBuf::from(val("--upload-dir")?)),
            "--services" => a.services = true,
            "--esphome" => a.esphome = Some(val("--esphome")?.parse().map_err(|_| "--esphome needs seconds".to_owned())?),
            "--esphome-send" => a.esphome_send.push(val("--esphome-send")?),
            "-V" | "--version" => {
                println!("smhubd {VERSION}");
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other}\n\n{HELP}")),
        }
    }
    Ok(a)
}

fn under(root: &Path, abs: &str) -> PathBuf {
    if root == Path::new("/") { PathBuf::from(abs) } else { root.join(abs.trim_start_matches('/')) }
}

/// Tell OpenRC's supervisor that the daemon is ready (`notify="socket:ready"`).
fn notify_ready() {
    if let Ok(path) = std::env::var("NOTIFY_SOCKET") {
        match std::os::unix::net::UnixDatagram::unbound() {
            Ok(s) => {
                if let Err(e) = s.send_to(b"READY=1\n", &path) {
                    tracing::warn!("readiness: {e}");
                }
            }
            Err(e) => tracing::warn!("readiness: {e}"),
        }
    }
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let term = signal(SignalKind::terminate());
    let int = signal(SignalKind::interrupt());
    match (term, int) {
        (Ok(mut t), Ok(mut i)) => {
            tokio::select! {
                _ = t.recv() => {}
                _ = i.recv() => {}
            }
        }
        _ => std::future::pending::<()>().await,
    }
    tracing::info!("stopping");
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if args.services {
        let sys = Sys::new(&args.root, args.bin_dir.as_deref());
        let started = std::time::Instant::now();
        let all = sys.read_services();
        let took = started.elapsed();
        for (name, state) in all {
            println!("{name} = {}", state.as_str());
        }
        eprintln!("read in {} us", took.as_micros());
        return;
    }
    if let Some(secs) = args.esphome {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("runtime: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = rt.block_on(esphome_probe(&args, secs)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_target(false).with_writer(std::io::stderr).init();
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(4).thread_keep_alive(Duration::from_secs(10)).build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(run(args)) {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

/// `--esphome`: watch ESPHome's API from the command line, with the key the hub stores.
async fn esphome_probe(args: &Args, secs: u64) -> Result<(), String> {
    use smhub_sys::esphome::{self, Command, Event, LightCommand, State};
    let db = args.db.clone().unwrap_or_else(|| under(&args.root, "/opt/smhub-services/config/backend.db"));
    let own = db.with_file_name("smhubd.db");
    let up = smhub_store::Upgrade { zigbee_adapter: "zstack", rtos_key: smhub_sys::rtos::FACTORY_KEY_B64 };
    let store = Store::open(&db, &own, &up).map_err(|e| format!("database: {e}"))?;
    let page = store.run(|db| db.page_settings("esphome")).await.map_err(|e| e.to_string())?;
    let key_b64 = page.get("encryption_key").cloned().unwrap_or_else(|| smhub_sys::rtos::FACTORY_KEY_B64.to_owned());
    let key = esphome::key_from_b64(&key_b64).ok_or("the stored key is not 32 bytes")?;
    let (ctx, crx) = tokio::sync::mpsc::channel(esphome::COMMANDS);
    let (etx, mut erx) = tokio::sync::mpsc::channel(esphome::EVENTS);
    let link = tokio::spawn(esphome::run(esphome::ADDR.to_owned(), key, crx, etx));
    let started = std::time::Instant::now();
    let deadline = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(deadline);
    let mut entities: Vec<esphome::Entity> = Vec::new();
    let mut sent = false;
    loop {
        tokio::select! {
            () = &mut deadline => break,
            ev = erx.recv() => {
                let Some(ev) = ev else { break };
                let at = started.elapsed().as_millis();
                match ev {
                    Event::Connected { name, server } => println!("{at:>6} ms connected: {name} ({server})"),
                    Event::Disconnected(e) => println!("{at:>6} ms disconnected: {e}"),
                    Event::Entities(list) => {
                        for e in &list {
                            println!("{at:>6} ms entity {:?} key={:#010x} object_id={} name={:?} effects={:?} args={:?}", e.kind, e.key, e.object_id, e.name, e.effects, e.args);
                        }
                        entities = list;
                        if !sent {
                            sent = true;
                            let find = |n: &str| entities.iter().find(|e| e.object_id == n || e.name == n).map(|e| e.key);
                            for spec in &args.esphome_send {
                                let parts: Vec<&str> = spec.splitn(4, ':').collect();
                                let cmd = match parts.as_slice() {
                                    ["light", n, "off"] => find(n).map(|key| Command::Light(LightCommand { key, on: Some(false), ..Default::default() })),
                                    ["light", n, rgb, rest @ ..] => {
                                        let c: Vec<f32> = rgb.split(',').filter_map(|x| x.parse().ok()).collect();
                                        find(n).filter(|_| c.len() == 3).map(|key| Command::Light(LightCommand {
                                            key,
                                            on: Some(true),
                                            brightness: Some(0.6),
                                            rgb: Some([c[0], c[1], c[2]]),
                                            effect: Some(rest.first().map_or("None", |e| *e).to_owned()),
                                            ..Default::default()
                                        }))
                                    }
                                    ["switch", n, v] => find(n).map(|key| Command::Switch { key, on: *v == "on" }),
                                    ["service", n, text] => find(n).map(|key| Command::Service { key, strings: vec![(*text).to_owned()] }),
                                    _ => None,
                                };
                                match cmd {
                                    Some(c) => {
                                        println!("{at:>6} ms send {c:?}");
                                        let _ = ctx.send(c).await;
                                    }
                                    None => println!("{at:>6} ms cannot send {spec}: no such entity or bad form"),
                                }
                            }
                        }
                    }
                    Event::State(s) => {
                        let name = entities.iter().find(|e| e.key == s.key()).map_or("?", |e| e.name.as_str());
                        match s {
                            State::Light { on, brightness, rgb, effect, .. } => println!("{at:>6} ms state {name}: on={on} brightness={brightness:.2} rgb={rgb:?} effect={effect:?}"),
                            State::Switch { on, .. } => println!("{at:>6} ms state {name}: on={on}"),
                            State::BinarySensor { on, missing, .. } => println!("{at:>6} ms state {name}: on={on} missing={missing}"),
                        }
                    }
                }
            }
        }
    }
    drop(ctx);
    let _ = tokio::time::timeout(Duration::from_secs(2), link).await;
    Ok(())
}

async fn run(args: Args) -> Result<(), String> {
    let started = std::time::Instant::now();
    let db = args.db.clone().unwrap_or_else(|| under(&args.root, "/opt/smhub-services/config/backend.db"));
    let own = db.with_file_name("smhubd.db");
    let sys = Arc::new(Sys::new(&args.root, args.bin_dir.as_deref()));
    // what the old migrations read from the hub (reference §19)
    let adapter = if sys.eeprom().is_some_and(|e| e.model.contains("mg")) { "ember" } else { "zstack" };
    let up = smhub_store::Upgrade { zigbee_adapter: adapter, rtos_key: smhub_sys::rtos::FACTORY_KEY_B64 };
    let store = Store::open(&db, &own, &up).map_err(|e| format!("database: {e}"))?;
    let upload_dir = args.upload_dir.clone().unwrap_or_else(|| {
        let user = under(&args.root, "/mnt/user");
        if user.is_dir() { user.join(".smhubd") } else { under(&args.root, "/tmp/.smhubd") }
    });
    let hub = Hub::new(sys, store, Config { version: VERSION.into(), upload_dir });
    smhub_api::auth::load(&hub).await.map_err(|e| format!("sessions: {e}"))?;

    let tcp = tokio::net::TcpListener::bind(&args.listen).await.map_err(|e| format!("listen {}: {e}", args.listen))?;
    let app = smhub_api::router(Arc::clone(&hub));
    let mut tasks = Vec::new();
    if let Some(sock) = &args.socket {
        let _ = std::fs::remove_file(sock);
        match tokio::net::UnixListener::bind(sock) {
            Ok(l) => {
                let device = smhub_sys::fsx::user_ids(&hub.sys.p("/etc/passwd"), smhub_sys::DEVICE_USER);
                let mut allowed = vec![0, nix::unistd::getuid().as_raw()];
                if let Some((uid, gid, _)) = device {
                    allowed.push(uid);
                    let _ = std::os::unix::fs::chown(sock, None, Some(gid));
                }
                let _ = std::fs::set_permissions(sock, std::os::unix::fs::PermissionsExt::from_mode(0o660));
                let local = smhub_api::local_router(Arc::clone(&hub));
                let listener = smhub_api::LocalListener { inner: l, allowed };
                tasks.push(tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, local.into_make_service()).await {
                        tracing::warn!("local socket: {e}");
                    }
                }));
            }
            Err(e) => tracing::warn!("local socket {}: {e}", sock.display()),
        }
    }
    notify_ready();
    tracing::info!("smhubd {VERSION} ready on {} after {} ms", args.listen, started.elapsed().as_millis());

    let h = Arc::clone(&hub);
    tokio::spawn(async move {
        if let Err(e) = smhub_api::apps::sync(&h, None).await {
            tracing::warn!("app sync: {e}");
        }
        if let Err(e) = smhub_api::apps::sync_stunnel(&h).await {
            tracing::warn!("stunnel: {e}");
        }
        smhub_api::routes::esphome_sync(&h).await;
        smhub_api::watchers::start(&h);
        smhub_api::hw::start(&h).await;
    });

    axum::serve(tcp, app.into_make_service()).with_graceful_shutdown(shutdown_signal()).await.map_err(|e| format!("server: {e}"))?;
    for t in tasks {
        t.abort();
    }
    if let Some(sock) = &args.socket {
        let _ = std::fs::remove_file(sock);
    }
    Ok(())
}

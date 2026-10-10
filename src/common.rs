use clap::App;
use hbb_common::{
    allow_err, anyhow::{Context, Result}, get_version_number, log, tokio, ResultType
};
use ini::Ini;
use sodiumoxide::crypto::sign;
use std::{
    io::prelude::*,
    io::Read,
    net::SocketAddr,
    time::{Instant, SystemTime},
};

#[allow(dead_code)]
pub(crate) fn get_expired_time() -> Instant {
    let now = Instant::now();
    now.checked_sub(std::time::Duration::from_secs(3600))
        .unwrap_or(now)
}

/// Secondary log writer that keeps an append-only, never-rotated copy of
/// WARN-and-above log lines, so errors and alerts survive the 7-file
/// rotation of the regular log. Raise the level check to `Error` only if
/// warnings should stay in the regular log only.
struct ErrorLevelWriter {
    inner: flexi_logger::writers::FileLogWriter,
}

impl flexi_logger::writers::LogWriter for ErrorLevelWriter {
    fn write(
        &self,
        now: &mut flexi_logger::DeferredNow,
        record: &log::Record,
    ) -> std::io::Result<()> {
        if record.level() <= log::Level::Warn {
            self.inner.write(now, record)
        } else {
            Ok(())
        }
    }

    fn flush(&self) -> std::io::Result<()> {
        self.inner.flush()
    }

    fn shutdown(&self) {
        self.inner.shutdown()
    }
}

/// Write logs to a `logs` directory next to the executable, rotated daily
/// with 7 files kept, so log retention no longer depends on the process
/// launcher's stdout redirection (e.g. nssm or journald). WARN-and-above
/// lines are additionally kept in a permanent `<name>_error.log`.
/// The returned handle must be kept alive for the process lifetime;
/// dropping it shuts the logger down.
#[allow(dead_code)]
pub fn setup_logger(name: &str) -> ResultType<flexi_logger::LoggerHandle> {
    let log_dir = std::env::current_exe()?
        .parent()
        .map(|d| d.to_path_buf())
        .unwrap_or_default()
        .join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let error_writer = flexi_logger::writers::FileLogWriter::builder(
            flexi_logger::FileSpec::default()
                .directory(log_dir.clone())
                .basename(format!("{name}_error"))
                .suffix("log")
                .suppress_timestamp(),
        )
        .format(flexi_logger::opt_format)
        .append()
        .try_build()?;
    let _logger = flexi_logger::Logger::try_with_env_or_str("info")?
        .log_to_file_and_writer(
            flexi_logger::FileSpec::default()
                .directory(log_dir)
                .basename(name.to_owned())
                .suffix("log"),
            Box::new(ErrorLevelWriter {
                inner: error_writer,
            }),
        )
        .rotate(
            flexi_logger::Criterion::Age(flexi_logger::Age::Day),
            flexi_logger::Naming::Timestamps,
            flexi_logger::Cleanup::KeepLogFiles(7),
        )
        .format(flexi_logger::opt_format)
        .write_mode(flexi_logger::WriteMode::BufferAndFlush)
        .start()?;
    Ok(_logger)
}

#[cfg(test)]
mod logger_tests {
    use hbb_common::log;

    #[test]
    fn setup_logger_routes_warnings_to_error_file() {
        let dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("logs");
        let _ = std::fs::remove_dir_all(&dir);
        {
            // Dropping the handle at scope end flushes and closes the files.
            let _handle = super::setup_logger("logtest").unwrap();
            log::error!("ERROR_MARKER_123");
            log::warn!("WARN_MARKER_456");
            log::info!("INFO_MARKER_789");
        }
        let err_log = std::fs::read_to_string(dir.join("logtest_error.log")).unwrap();
        assert!(err_log.contains("ERROR_MARKER_123"));
        assert!(err_log.contains("WARN_MARKER_456"));
        assert!(!err_log.contains("INFO_MARKER_789"));
        let main_log = std::fs::read_to_string(dir.join("logtest_rCURRENT.log")).unwrap();
        assert!(main_log.contains("ERROR_MARKER_123"));
        assert!(main_log.contains("WARN_MARKER_456"));
        assert!(main_log.contains("INFO_MARKER_789"));
    }
}

#[allow(dead_code)]
pub(crate) fn test_if_valid_server(host: &str, name: &str) -> ResultType<SocketAddr> {
    use std::net::ToSocketAddrs;
    let res = if host.contains(':') {
        host.to_socket_addrs()?.next().context("")
    } else {
        format!("{}:{}", host, 0)
            .to_socket_addrs()?
            .next()
            .context("")
    };
    if res.is_err() {
        log::error!("Invalid {} {}: {:?}", name, host, res);
    }
    res
}

#[allow(dead_code)]
pub(crate) fn get_servers(s: &str, tag: &str) -> Vec<String> {
    let servers: Vec<String> = s
        .split(',')
        .filter(|x| !x.is_empty() && test_if_valid_server(x, tag).is_ok())
        .map(|x| x.to_owned())
        .collect();
    log::info!("{}={:?}", tag, servers);
    servers
}

#[allow(dead_code)]
#[inline]
fn arg_name(name: &str) -> String {
    name.to_uppercase().replace('_', "-")
}

#[allow(dead_code)]
pub fn init_args(args: &str, name: &str, about: &str) {
    let matches = App::new(name)
        .version(crate::version::VERSION)
        .author("Purslane Ltd. <info@rustdesk.com>")
        .about(about)
        .args_from_usage(args)
        .get_matches();
    if let Ok(v) = Ini::load_from_file(".env") {
        if let Some(section) = v.section(None::<String>) {
            section
                .iter()
                .for_each(|(k, v)| std::env::set_var(arg_name(k), v));
        }
    }
    if let Some(config) = matches.value_of("config") {
        if let Ok(v) = Ini::load_from_file(config) {
            if let Some(section) = v.section(None::<String>) {
                section
                    .iter()
                    .for_each(|(k, v)| std::env::set_var(arg_name(k), v));
            }
        }
    }
    for (k, v) in matches.args {
        if let Some(v) = v.vals.first() {
            std::env::set_var(arg_name(k), v.to_string_lossy().to_string());
        }
    }
}

#[allow(dead_code)]
#[inline]
pub fn get_arg(name: &str) -> String {
    get_arg_or(name, "".to_owned())
}

#[allow(dead_code)]
#[inline]
pub fn get_arg_or(name: &str, default: String) -> String {
    std::env::var(arg_name(name)).unwrap_or(default)
}

#[allow(dead_code)]
#[inline]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|x| x.as_secs())
        .unwrap_or_default()
}

pub fn gen_sk(wait: u64) -> (String, Option<sign::SecretKey>) {
    let sk_file = "id_ed25519";
    if wait > 0 && !std::path::Path::new(sk_file).exists() {
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }
    if let Ok(mut file) = std::fs::File::open(sk_file) {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            let contents = contents.trim();
            let sk = base64::decode(contents).unwrap_or_default();
            if sk.len() == sign::SECRETKEYBYTES {
                let mut tmp = [0u8; sign::SECRETKEYBYTES];
                tmp[..].copy_from_slice(&sk);
                let pk = base64::encode(&tmp[sign::SECRETKEYBYTES / 2..]);
                log::info!("Private key comes from {}", sk_file);
                return (pk, Some(sign::SecretKey(tmp)));
            } else {
                // don't use log here, since it is async
                println!("Fatal error: malformed private key in {sk_file}.");
                std::process::exit(1);
            }
        }
    } else {
        let gen_func = || {
            let (tmp, sk) = sign::gen_keypair();
            (base64::encode(tmp), sk)
        };
        let (mut pk, mut sk) = gen_func();
        for _ in 0..300 {
            if !pk.contains('/') && !pk.contains(':') {
                break;
            }
            (pk, sk) = gen_func();
        }
        let pub_file = format!("{sk_file}.pub");
        if let Ok(mut f) = std::fs::File::create(&pub_file) {
            f.write_all(pk.as_bytes()).ok();
            if let Ok(mut f) = std::fs::File::create(sk_file) {
                let s = base64::encode(&sk);
                if f.write_all(s.as_bytes()).is_ok() {
                    log::info!("Private/public key written to {}/{}", sk_file, pub_file);
                    log::debug!("Public key: {}", pk);
                    return (pk, Some(sk));
                }
            }
        }
    }
    ("".to_owned(), None)
}

#[cfg(unix)]
pub async fn listen_signal() -> Result<()> {
    use hbb_common::tokio;
    use hbb_common::tokio::signal::unix::{signal, SignalKind};

    tokio::spawn(async {
        let mut s = signal(SignalKind::terminate())?;
        let terminate = s.recv();
        let mut s = signal(SignalKind::interrupt())?;
        let interrupt = s.recv();
        let mut s = signal(SignalKind::quit())?;
        let quit = s.recv();

        tokio::select! {
            _ = terminate => {
                log::info!("signal terminate");
            }
            _ = interrupt => {
                log::info!("signal interrupt");
            }
            _ = quit => {
                log::info!("signal quit");
            }
        }
        Ok(())
    })
    .await?
}

#[cfg(not(unix))]
pub async fn listen_signal() -> Result<()> {
    let () = std::future::pending().await;
    unreachable!();
}


pub fn check_software_update() {
    const ONE_DAY_IN_SECONDS: u64 = 60 * 60 * 24;
    std::thread::spawn(move || loop {
        std::thread::spawn(move || allow_err!(check_software_update_()));
        std::thread::sleep(std::time::Duration::from_secs(ONE_DAY_IN_SECONDS));
    });
}

#[tokio::main(flavor = "current_thread")]
async fn check_software_update_() -> hbb_common::ResultType<()> {
    let (request, url) = hbb_common::version_check_request(hbb_common::VER_TYPE_RUSTDESK_SERVER.to_string());
    let latest_release_response = reqwest::Client::builder().build()?
        .post(url)
        .json(&request)
        .send()
        .await?;

    let bytes = latest_release_response.bytes().await?;
    let resp: hbb_common::VersionCheckResponse = serde_json::from_slice(&bytes)?;
    let response_url = resp.url;
    let latest_release_version = response_url.rsplit('/').next().unwrap_or_default();
    if get_version_number(&latest_release_version) > get_version_number(crate::version::VERSION) {
       log::info!("new version is available: {}", latest_release_version);
    }
    Ok(())
}
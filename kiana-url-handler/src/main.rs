use kiana_url_handler::run_mcp_server;
#[cfg(target_os = "linux")]
use kiana_url_handler::UrlHandler;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("mcp") | Some("serve") => run_mcp_server().await.map_err(|e| e.to_string()),
        Some("register") => {
            let scheme = args.get(1).ok_or_else(|| usage())?;
            register_scheme(scheme)
        }
        Some("status") => {
            let scheme = args.get(1).ok_or_else(|| usage())?;
            print_registration_status(scheme)
        }
        Some("handle") => {
            let url = args.get(1).ok_or_else(|| usage())?;
            Url::parse(url).map_err(|e| format!("invalid URL: {}", e))?;
            println!("{}", url);
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            println!("{}", usage());
            Ok(())
        }
        Some(_) => Err(usage()),
    }
}

#[cfg(target_os = "linux")]
fn register_scheme(scheme: &str) -> Result<(), String> {
    let path = UrlHandler::register_linux_scheme_with_current_exe(scheme)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn register_scheme(_scheme: &str) -> Result<(), String> {
    Err("direct scheme registration is only implemented on Linux".to_string())
}

#[cfg(target_os = "linux")]
fn print_registration_status(scheme: &str) -> Result<(), String> {
    println!("{}", UrlHandler::linux_registration_status(scheme)?);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn print_registration_status(_scheme: &str) -> Result<(), String> {
    Err("registration status is only implemented on Linux".to_string())
}

fn usage() -> String {
    "Usage: kiana-url-handler [mcp|register <scheme>|status <scheme>|handle <url>]".to_string()
}

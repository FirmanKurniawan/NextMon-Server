//! Manual test CLI.
//!   cargo run --example rt -- --root X:\nm-rt-test <command> [args]
//! Commands: available <php|node> [channel] | installed <php|node> | install <php|node> <spec> [ts]
//!           uninstall <php|node> <version> [ts] | health <php|node> <version> [ts] | cacert
use nm_runtimes::*;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = "runtimes".to_string();
    if args.first().map(|s| s == "--root").unwrap_or(false) {
        args.remove(0);
        root = args.remove(0);
    }
    if args.len() < 1 {
        eprintln!("usage: rt [--root DIR] available|installed|install|uninstall|health|cacert <php|node> ...");
        std::process::exit(2);
    }
    let mgr = RuntimeManager::new(&root, Options::default()).expect("manager");
    let cmd = args[0].as_str();
    if cmd == "cacert" {
        println!("{:?}", mgr.ensure_cacert().await);
        return;
    }
    let kind = args.get(1).and_then(|s| RuntimeKind::parse(s)).unwrap_or_else(|| {
        eprintln!("need kind php|node");
        std::process::exit(2)
    });
    let a2 = args.get(2).cloned().unwrap_or_default();
    let ts = args.iter().skip(2).any(|s| s == "ts");
    let variant = if ts { Variant::Ts } else { Variant::Nts };
    let res: Result<serde_json::Value> = async {
        Ok(match cmd {
            "available" => {
                let ch = if a2.is_empty() { if kind == RuntimeKind::Node { Channel::Lts } else { Channel::Stable } } else { Channel::parse(&a2).unwrap_or_default() };
                serde_json::to_value(mgr.overview(kind, ch, variant, false).await?)?
            }
            "installed" => serde_json::to_value(mgr.list_installed(kind))?,
            "install" => {
                let last = std::sync::Mutex::new(-1i64);
                let p: ProgressFn = Arc::new(move |p| {
                    let pct = (p.progress * 100.0) as i64;
                    let mut l = last.lock().unwrap();
                    if pct != *l {
                        *l = pct;
                        eprint!("\r[{:>3}%] {:?} {}                    ", pct, p.step, p.message.chars().take(60).collect::<String>());
                    }
                });
                let r = mgr.install(kind, &a2, variant, p, CancelToken::new()).await?;
                eprintln!();
                serde_json::to_value(r)?
            }
            "uninstall" => {
                mgr.uninstall(kind, &a2, variant, &|_| false)?;
                serde_json::json!({"ok": true})
            }
            "health" => serde_json::to_value(mgr.health_check(kind, &a2, variant).await?)?,
            _ => return Err(Error::Unsupported(format!("command {cmd}"))),
        })
    }
    .await;
    match res {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            println!("{}", serde_json::to_string_pretty(&e.to_api()).unwrap());
            std::process::exit(1);
        }
    }
}

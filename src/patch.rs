// 镜像源服务器根地址
// gh-proxy.com（Cloudflare v4 优选加速节点，仅走 IPv4 智能解析）
const GH_PROXY_HOST: &str = "v4.gh-proxy.org";
const GH_PROXY_TEXT_ROOT: &str = "https://v4.gh-proxy.org/https://raw.githubusercontent.com/solsticegamestudios/GModPatchTool/refs/heads/files/";
const GH_PROXY_BINARY_ROOT: &str = "https://v4.gh-proxy.org/https://github.com/solsticegamestudios/GModPatchTool/raw/refs/heads/files/";

// scs.mf.nekogan.com（镜像站备用源）
const NEKOGAN_TEXT_ROOT: &str = "https://scs.mf.nekogan.com/raw/";
const NEKOGAN_BINARY_ROOT: &str = "https://scs.mf.nekogan.com/media/";

// 启动测速：用一个小补丁文件检测二进制源是否真的可用，并测量下载速度
const SPEED_TEST_TEXT_FILE: &str = "manifest.json";
const SPEED_TEST_BINARY_FILE: &str = "patches/windows/dev/bin/chrome_100_percent.pak.bsdiff";
const SPEED_TEST_MAX_BYTES: usize = 2 * 1024 * 1024; // 每个源最多测 2 MiB
const GOOD_SPEED_BYTES_PER_SEC: u64 = 1024 * 1024; // 达到 1 MiB/s 视为速度理想
const SPEED_TEST_TIMEOUT: time::Duration = time::Duration::from_secs(8);


//const GMOD_STEAM_APPID: u64 = 4000;
const BLANK_FILE_HASH: &str = "null";

// Spinner: a 2x4 Braille well that fills bottom-up with uniform 1-row block drops (3DS download vibe)
// The full well holds for two ticks before the reset beat; the last frame is indicatif's finished state
// The reset frame stays escaped because U+2800 (Braille blank) is invisible in source
const SPINNER_FRAMES: &[&str] = &[
	"⠄", "⡀", "⡠", "⣀", "⣂", "⣄", "⣔",
	"⣤", "⣥", "⣦", "⣮", "⣶", "⣶", "\u{2800}", "⣶",
];
const SPINNER_TICK: time::Duration = time::Duration::from_millis(100);

use crate::*;

use serde::Deserialize;
use tracing::error;
use tracing_subscriber::filter::EnvFilter;
use clap::Parser;
use std::io::IsTerminal;
use phf::phf_map;
use phf::Map;
use std::time;
use steamid::SteamId;
use sysinfo::System;
use std::fs::File;
use std::io;
use std::net::SocketAddr;
use bytes::Bytes;
use reqwest::Response;
use tokio::time::Instant;
use tokio::task::JoinSet;
use qbsdiff::Bspatch;
use regex::Regex;
use std::sync::OnceLock;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle, ProgressDrawTarget};

use super::vdf;

// Live progress UI; bars register here so terminal_write can suspend them while printing
static UI: OnceLock<MultiProgress> = OnceLock::new();

fn spinner_style(template: &str) -> ProgressStyle {
	ProgressStyle::with_template(template).unwrap().tick_strings(SPINNER_FRAMES).progress_chars("#>-")
}

#[cfg(windows)]
use is_elevated::is_elevated;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Parser)]
#[command(version)]
struct Args {
	/// Launch Garry's Mod after successfully patching
	#[arg(short, long)]
	launch_gmod: bool,

	/// Skip "Press Enter to exit..." on tool exit
	#[arg(short, long)]
	skip_exit_prompt: bool,

	/// Force a specific Steam install path (NOT a Steam library path)
	#[arg(long)]
	steam_path: Option<PathBuf>,

	/// Don't apply SourceScheme (VGUI Theme) changes
	#[arg(long)]
	no_sourcescheme: bool,

	/// Skip deleting ChromiumCache/ChromiumCacheMultirun/chromium.log from the GarrysMod directory
	#[arg(long)]
	skip_clear_chromiumcache: bool,

	/// Skip pre-warming Rosetta translations of patched libraries on Apple Silicon
	#[cfg(target_os = "macos")]
	#[arg(long)]
	skip_rosetta_prewarm: bool,

	/// Force redownload all patch files from scratch and clear the GModPatchTool cache directory on exit
	#[arg(long)]
	disable_cache: bool,

	/// Don't use the OS proxy configuration for HTTP requests
	#[arg(long)]
	no_system_proxy: bool,

	/// Apply patches even if Garry's Mod is currently running (may cause issues!)
	#[arg(long)]
	ignore_gmod_running: bool,

	/// Allow running the tool as root/admin (NOT RECOMMENDED!!!)
	#[arg(long)]
	run_as_root_with_security_risk: bool
}

const COLOR_LOOKUP: Map<&'static str, &'static str> =
phf_map! {
	"red" => "\x1B[1;31m",
	"green" => "\x1B[1;32m",
	"yellow" => "\x1B[1;33m",
	"magenta" => "\x1B[1;35m",
	"cyan" => "\x1B[1;36m"
};

use thiserror::Error;
#[derive(Debug, Error)]
enum AlmightyError {
	#[error("HTTP 错误: {0}")]
	Http(#[from] reqwest::Error),
	#[error("远程版本号解析出错: {0}")]
	Parse(#[from] std::num::ParseIntError),
	#[error("{0}")]
	Generic(String)
}

// VDF structs
//
// Steam/config/loginusers.vdf
//
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUser {
	#[serde(alias = "accountname")]
	account_name: String,
	#[serde(alias = "personaname")]
	persona_name: String,
	//remember_password: bool,
	//wants_offline_mode: bool,
	//skip_offline_mode_warning: bool,
	//allow_auto_login: bool,
	#[serde(alias = "mostrecent", default)]
	most_recent: bool,
	#[serde(alias = "timestamp", default)]
	timestamp: u64 // Y2K38
}

//
// Steam/config/libraryfolders.vdf
//
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamLibraryFolder {
	#[serde(alias = "path")]
	path: String,
	//label: String,
	//contentid: i64,
	//totalsize: u64,
	//update_clean_bytes_tally: u64,
	//time_last_update_verified: u64,
	//#[serde(alias = "apps")]
	//apps: SteamLibraryFolderApps
}

// Steam adds plain number values here too (e.g. contentstatsid), so tolerate anything that isn't a folder
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum SteamLibraryFolderEntry {
	Folder(SteamLibraryFolder),
	Other(serde::de::IgnoredAny)
}

//#[derive(Deserialize, Debug)]
//struct SteamLibraryFolderApps {
//	#[serde(rename = "4000")]
//	gmod: Option<u64>
//}

//
// SteamLibrary/appmanifest_4000.acf
//
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamAppManifest {
	//appid: u64,
	//universe: u8, // 0-5
	//launcher_path: String,
	//name: String,
	#[serde(alias = "stateflags")]
	state_flags: u32, // https://github.com/SteamDatabase/SteamTracking/blob/master/Structs/EAppState.json
	#[serde(alias = "installdir")]
	install_dir: String,
	//last_updated: u64,
	//last_played: u64,
	//size_on_disk: u64,
	//buildid: u32,
	//last_owner: u64,
	//download_type: u32, // TODO: Is this right? Can't find documentation anywhere
	//update_result: u32, // TODO: Is this right? Can't find documentation anywhere
	#[serde(alias = "bytestodownload", default)]
	bytes_to_download: u64,
	#[serde(alias = "bytesdownloaded", default)]
	bytes_downloaded: u64,
	#[serde(alias = "bytestostage", default)]
	bytes_to_stage: u64,
	#[serde(alias = "bytesstaged", default)]
	bytes_staged: u64,
	//target_build_id: u32,
	//auto_update_behavior: u8, // 1-3
	//allow_other_downloads_while_running: bool,
	#[serde(alias = "scheduledautoupdate")]
	scheduled_auto_update: u64, // Y2K38
	#[serde(alias = "fullvalidatebeforenextupdate")]
	full_validate_before_next_update: Option<bool>,
	//full_validate_after_next_update: bool,
	//installed_depots: ,
	//shared_depots: ,
	//user_config: SteamAppConfig,
	#[serde(alias = "mountedconfig")]
	mounted_config: SteamAppConfig
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamAppConfig {
	#[serde(alias = "betakey")]
	beta_key: Option<String>,
	//language: Option<String>
}

//
// Steam/config/config.vdf
//
#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamConfig {
	#[serde(alias = "software")]
	software: SteamConfigSoftware
	// Several entries unimplemented!
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamConfigSoftware {
	#[serde(alias = "valve")]
	valve: SteamConfigValve
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamConfigValve {
	#[serde(alias = "steam")]
	steam: SteamConfigSteam
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamConfigSteam {
	#[serde(alias = "compattoolmapping")]
	compat_tool_mapping: Option<SteamConfigCompatToolMappingApps>
	// Several entries unimplemented!
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
struct SteamConfigCompatToolMappingApps {
	#[serde(rename = "4000")]
	gmod: Option<SteamCompatToolMapping>
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamCompatToolMapping {
	#[serde(alias = "name")]
	name: String,
	//config: ,
	//priority:
}

//
// Steam/userdata/<steamid u32>/config/localconfig.vdf
//
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUserLocalConfig {
	#[serde(alias = "software")]
	software: SteamUserLocalConfigSoftware,
	// Several entries unimplemented!
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUserLocalConfigSoftware {
	#[serde(alias = "valve")]
	valve: SteamUserLocalConfigValve
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUserLocalConfigValve {
	#[serde(alias = "steam")]
	steam: SteamUserLocalConfigSteam
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUserLocalConfigSteam {
	#[serde(alias = "apps")]
	apps: SteamUserLocalConfigApps
	// Several entries unimplemented!
}

#[derive(Deserialize, Debug)]
struct SteamUserLocalConfigApps {
	#[serde(rename = "4000")]
	gmod: Option<SteamUserLocalConfigApp>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SteamUserLocalConfigApp {
	//last_played: u64,
	//playtime: u32,
	//cloud: ,
	//<appid>_eula_0: ,
	//<appid>_eula_1: ,
	//autocloud: ,
	//badge_data: ,
	#[serde(alias = "launchoptions")]
	launch_options: Option<String>,
	//playtime2wks: u16
}

fn terminal_write<W>(writer: fn() -> W, output: &str, newline: bool, color: Option<&str>)
where
	W: std::io::Write + 'static
{
	let write_now = || {
		if let Some(color) = color && COLOR_LOOKUP.contains_key(color) {
			write!(writer(), "{}", COLOR_LOOKUP[color]).unwrap();
		}

		if newline {
			writeln!(writer(), "{output}").unwrap();
		} else {
			write!(writer(), "{output}").unwrap();
		}

		if color.is_some() {
			write!(writer(), "\x1B[0m").unwrap();
		}
	};

	// Suspend any live progress bars so the spinner line isn't clobbered
	if let Some(ui) = UI.get() {
		ui.suspend(write_now);
	} else {
		write_now();
	}
}

/// 启动时选出的 gh-proxy IPv4 优选节点；选好后所有请求都固定走这个 IP
static GH_PROXY_IPV4_NODE: OnceLock<SocketAddr> = OnceLock::new();

// 测速后可能的镜像源组合（按速度从快到慢排列）
static GH_PROXY_FIRST_TEXT_ROOTS: [&str; 2] = [GH_PROXY_TEXT_ROOT, NEKOGAN_TEXT_ROOT];
static NEKOGAN_FIRST_TEXT_ROOTS: [&str; 2] = [NEKOGAN_TEXT_ROOT, GH_PROXY_TEXT_ROOT];
static NEKOGAN_ONLY_TEXT_ROOTS: [&str; 1] = [NEKOGAN_TEXT_ROOT];
static GH_PROXY_FIRST_BINARY_ROOTS: [&str; 2] = [GH_PROXY_BINARY_ROOT, NEKOGAN_BINARY_ROOT];
static NEKOGAN_ONLY_BINARY_ROOTS: [&str; 1] = [NEKOGAN_BINARY_ROOT];

struct MirrorSelection {
	text_roots: &'static [&'static str],
	binary_roots: &'static [&'static str]
}

/// 解析域名下的全部 IPv4 地址（gh-proxy v4 优选节点仅使用 IPv4）
async fn resolve_ipv4_nodes(host: &str) -> Vec<SocketAddr> {
	match tokio::net::lookup_host((host, 443)).await {
		Ok(addresses) => addresses.filter(|address| address.is_ipv4()).collect(),
		Err(_) => Vec::new()
	}
}

/// 构建 HTTP 客户端；若已选出 gh-proxy IPv4 优选节点，则固定解析到该节点
fn http_client_builder(no_system_proxy: bool) -> reqwest::ClientBuilder {
	let mut builder = reqwest::Client::builder();

	if no_system_proxy {
		builder = builder.no_proxy();
	}

	if let Some(ipv4_node) = GH_PROXY_IPV4_NODE.get() {
		builder = builder.resolve(GH_PROXY_HOST, *ipv4_node);
	}

	builder
		.https_only(true) // Never follow a redirect down to plaintext HTTP
		.connect_timeout(std::time::Duration::new(10, 0)) // Initial connection failure
		.read_timeout(std::time::Duration::new(10, 0)) // Stall detection
}

/// 探测某个镜像源的下载速度；二进制源还会校验文件头，防止代理返回 LFS 指针之类的假文件
async fn probe_mirror_speed(root: &str, filename: &str, validate_binary: bool, no_system_proxy: bool, ipv4_node: Option<SocketAddr>) -> Option<u64> {
	let url = root.to_string() + filename;

	let mut builder = reqwest::Client::builder();

	if no_system_proxy {
		builder = builder.no_proxy();
	}

	if let Some(ipv4_node) = ipv4_node {
		builder = builder.resolve(GH_PROXY_HOST, ipv4_node);
	}

	let client = builder
		.https_only(true)
		.connect_timeout(std::time::Duration::new(10, 0))
		.read_timeout(std::time::Duration::new(10, 0))
		.build()
		.ok()?;

	let started = Instant::now();

	let probe_result = tokio::time::timeout(SPEED_TEST_TIMEOUT, async {
		let response = client.get(url).send().await.ok()?;

		if response.status().as_u16() != 200 {
			return None;
		}

		let mut response = response;
		let mut buf: Vec<u8> = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);

		while let Some(chunk) = response.chunk().await.ok().flatten() {
			buf.extend_from_slice(&chunk);

			if buf.len() >= SPEED_TEST_MAX_BYTES {
				break;
			}
		}

		if buf.is_empty() {
			return None;
		}

		// BSDIFF40 或 Zstd 魔数；都不是就说明源返回了假文件（例如 GitHub LFS 指针）
		if validate_binary && !(buf.starts_with(b"BSDIFF40") || buf.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])) {
			return None;
		}

		Some(buf.len() as u64)
	}).await;

	let received = match probe_result {
		Ok(Some(received)) => received,
		_ => return None
	};

	let elapsed = started.elapsed().as_secs_f64();

	if elapsed <= 0.0 {
		return None;
	}

	Some((received as f64 / elapsed) as u64)
}

/// 启动时自动检测各镜像源的下载速度：
/// gh-proxy（IPv4 优选节点）速度理想时优先使用，否则使用 scs.mf.nekogan.com
async fn select_mirror_sources<W>(writer: fn() -> W, writer_is_interactive: bool, no_system_proxy: bool) -> MirrorSelection
where
	W: std::io::Write + 'static
{
	terminal_write(writer, "\n正在自动检测镜像源下载速度...", true, None);

	// 解析 gh-proxy 的全部 IPv4 节点并逐个测速，选出最快节点
	let mut best_gh_proxy_node: Option<(SocketAddr, u64)> = None;

	for ipv4_node in resolve_ipv4_nodes(GH_PROXY_HOST).await {
		if let Some(speed) = probe_mirror_speed(GH_PROXY_TEXT_ROOT, SPEED_TEST_TEXT_FILE, false, no_system_proxy, Some(ipv4_node)).await {
			let is_faster = match best_gh_proxy_node {
				Some((_, best_speed)) => speed > best_speed,
				None => true
			};

			if is_faster {
				best_gh_proxy_node = Some((ipv4_node, speed));
			}
		}
	}

	let gh_proxy_text_speed = match best_gh_proxy_node {
		Some((ipv4_node, speed)) => {
			let _ = GH_PROXY_IPV4_NODE.set(ipv4_node);
			terminal_write(writer, format!("gh-proxy IPv4 优选节点: {ipv4_node}（{:.2} MiB/s）", speed as f64 / 0x100000 as f64).as_str(), true, if writer_is_interactive { Some("cyan") } else { None });
			Some(speed)
		},
		None => probe_mirror_speed(GH_PROXY_TEXT_ROOT, SPEED_TEST_TEXT_FILE, false, no_system_proxy, None).await
	};

	let gh_proxy_binary_speed = probe_mirror_speed(GH_PROXY_BINARY_ROOT, SPEED_TEST_BINARY_FILE, true, no_system_proxy, GH_PROXY_IPV4_NODE.get().copied()).await;
	let nekogan_text_speed = probe_mirror_speed(NEKOGAN_TEXT_ROOT, SPEED_TEST_TEXT_FILE, false, no_system_proxy, None).await;
	let nekogan_binary_speed = probe_mirror_speed(NEKOGAN_BINARY_ROOT, SPEED_TEST_BINARY_FILE, true, no_system_proxy, None).await;

	// 文本源（version.txt / manifest.json）：gh-proxy 速度理想且不慢于备用源时优先
	let gh_text_good = match (gh_proxy_text_speed, nekogan_text_speed) {
		(Some(gh_speed), Some(nekogan_speed)) => gh_speed >= GOOD_SPEED_BYTES_PER_SEC && gh_speed >= nekogan_speed,
		(Some(gh_speed), None) => gh_speed >= GOOD_SPEED_BYTES_PER_SEC,
		(None, _) => false
	};

	let text_roots: &'static [&'static str] = if gh_text_good {
		&GH_PROXY_FIRST_TEXT_ROOTS
	} else if gh_proxy_text_speed.is_some() {
		&NEKOGAN_FIRST_TEXT_ROOTS
	} else {
		&NEKOGAN_ONLY_TEXT_ROOTS
	};

	// 补丁源：只有 gh-proxy 能真正下载二进制文件且速度理想时才优先；
	// 否则只用 scs.mf.nekogan.com，避免代理返回 LFS 指针导致校验失败
	let gh_binary_good = match (gh_proxy_binary_speed, nekogan_binary_speed) {
		(Some(gh_speed), Some(nekogan_speed)) => gh_speed >= GOOD_SPEED_BYTES_PER_SEC && gh_speed >= nekogan_speed,
		(Some(gh_speed), None) => gh_speed >= GOOD_SPEED_BYTES_PER_SEC,
		(None, _) => false
	};

	let binary_roots: &'static [&'static str] = if gh_binary_good {
		&GH_PROXY_FIRST_BINARY_ROOTS
	} else {
		&NEKOGAN_ONLY_BINARY_ROOTS
	};

	let format_speed = |speed: Option<u64>| -> String {
		match speed {
			Some(speed) => format!("{:.2} MiB/s", speed as f64 / 0x100000 as f64),
			None => "不可用".to_string()
		}
	};

	terminal_write(writer, format!("gh-proxy.com（IPv4 优选）: 文本 {}，补丁 {}", format_speed(gh_proxy_text_speed), format_speed(gh_proxy_binary_speed)).as_str(), true, None);
	terminal_write(writer, format!("scs.mf.nekogan.com: 文本 {}，补丁 {}", format_speed(nekogan_text_speed), format_speed(nekogan_binary_speed)).as_str(), true, None);
	terminal_write(writer, format!("已选择文本源: {}", text_roots.iter().map(|root| root.to_string()).collect::<Vec<_>>().join(" -> ")).as_str(), true, if writer_is_interactive { Some("green") } else { None });
	terminal_write(writer, format!("已选择补丁源: {}", binary_roots.iter().map(|root| root.to_string()).collect::<Vec<_>>().join(" -> ")).as_str(), true, if writer_is_interactive { Some("green") } else { None });

	MirrorSelection {
		text_roots,
		binary_roots
	}
}

async fn get_http_response<W>(writer: fn() -> W, writer_is_interactive: bool, servers: &[&str], filename: &str, mut server_id: u8, mut try_count: u8, no_system_proxy: bool) -> Option<Response>
where
	W: std::io::Write + 'static
{
	let mut response = None;

	while (server_id as usize) < servers.len() {
		let url = servers[server_id as usize].to_string() + filename;

		let client = http_client_builder(no_system_proxy)
			//.timeout(std::time::Duration::new(size, 0)) // TODO: Total DEADLINE timeout (downloading too slow)
			.build();

		let response_result = match client {
			Ok(client) => client.get(url.clone()).send().await,
			Err(error) => Err(error)
		};

		match response_result {
			Ok(response_unwrapped) => {
				let response_status_code = response_unwrapped.status().as_u16();
				if response_status_code == 200 {
					response = Some(response_unwrapped);
					break;
				} else {
					terminal_write(writer, format!("\n{url}\n\t错误的 HTTP 状态码: {response_status_code}\n\n正在尝试切换下载源...").as_str(), true, if writer_is_interactive { Some("red") } else { None });
					response = None;
					server_id += 1;
					try_count = 0;
				}
			},
			Err(error) => {
				let error = error.without_url();
				terminal_write(writer, format!("\n{url}\n\tHTTP 出错: {error}\n\n当前重试次数：{try_count}。\n无需担心，连续失败三次后会自动切换备用源！").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				response = None;
				try_count += 1;

				// Try each server 3 times for full HTTP errors (Anti-DDoS, etc)
				if try_count >= 3 {
					server_id += 1;
					try_count = 0;
				}
			}
		}
	}

	response
}

// Make sure we have response data that *works*
// Otherwise we'll get "error decoding response body" or something later, where we don't have retry logic
// TODO: These could probably be DRY'd up...
async fn get_http_response_bytes<W>(writer: fn() -> W, writer_is_interactive: bool, bar: &ProgressBar, servers: &[&str], filename: &str, no_system_proxy: bool) -> Option<Bytes>
where
	W: std::io::Write + 'static
{
	let mut server_id: u8 = 0;
	let mut try_count: u8 = 0;
	let mut response_bytes = None;
	let mut length_counted = false;

	while (server_id as usize) < servers.len() {
		let response = get_http_response(writer, writer_is_interactive, servers, filename, server_id, try_count, no_system_proxy).await;

		if let Some(mut response) = response {
			// Count this file's size toward the bar total once, even across retries
			if !length_counted {
				bar.inc_length(response.content_length().unwrap_or(0));
				length_counted = true;
			}

			let mut buf: Vec<u8> = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
			let mut stream_error = None;

			// Stream chunks so we can show live download progress
			// TODO: Stream to disk instead of accumulating whole files in RAM
			loop {
				match response.chunk().await {
					Ok(Some(chunk)) => {
						bar.inc(chunk.len() as u64);
						buf.extend_from_slice(&chunk);
					},
					Ok(None) => break,
					Err(error) => {
						stream_error = Some(error);
						break;
					}
				}
			}

			match stream_error {
				// TODO: Check if buf is empty as well
				None => {
					response_bytes = Some(Bytes::from(buf));
					break;
				},
				Some(error) => {
					terminal_write(writer, format!("\nHTTP 字节错误: {error}\n\n当前重试次数：{try_count}。\n无需担心，连续失败三次后会自动切换备用源！").as_str(), true, if writer_is_interactive { Some("red") } else { None });
					response_bytes = None;
					try_count += 1;

					// Try each server 3 times
					if try_count >= 3 {
						server_id += 1;
						try_count = 0;
					}
				}
			}
		} else {
			// get_http_response only returns None once every server is exhausted, so stop instead of looping forever
			break;
		}
	}

	response_bytes
}

async fn get_http_response_text<W>(writer: fn() -> W, writer_is_interactive: bool, servers: &[&str], filename: &str, no_system_proxy: bool) -> Option<String>
where
	W: std::io::Write + 'static
{
	let mut server_id: u8 = 0;
	let mut try_count: u8 = 0;
	let mut response_text = None;

	while (server_id as usize) < servers.len() {
		let response = get_http_response(writer, writer_is_interactive, servers, filename, server_id, try_count, no_system_proxy).await;

		if let Some(response) = response {
			let response_text_raw = response.text().await;

			match response_text_raw {
				// TODO: Check if Text is empty as well
				Ok(response_text_raw) => {
					response_text = Some(response_text_raw);
					break;
				},
				Err(error) => {
					terminal_write(writer, format!("\nHTTP 文本错误: {error}\n\n当前重试次数：{try_count}。\n无需担心，连续失败三次后会自动切换备用源！").as_str(), true, if writer_is_interactive { Some("red") } else { None });
					response_text = None;
					try_count += 1;

					// Try each server 3 times
					if try_count >= 3 {
						server_id += 1;
						try_count = 0;
					}
				}
			}
		} else {
			// get_http_response only returns None once every server is exhausted, so stop instead of looping forever
			break;
		}
	}

	response_text
}

async fn get_http_response_json<W, T>(writer: fn() -> W, writer_is_interactive: bool, servers: &[&str], filename: &str, no_system_proxy: bool) -> Option<T>
where
	W: std::io::Write + 'static,
	T: serde::de::DeserializeOwned
{
	let mut server_id: u8 = 0;
	let mut try_count: u8 = 0;
	let mut response_json = None;

	while (server_id as usize) < servers.len() {
		let response = get_http_response(writer, writer_is_interactive, servers, filename, server_id, try_count, no_system_proxy).await;

		if let Some(response) = response {
			let response_json_raw = response.json::<T>().await;

			match response_json_raw {
				// TODO: Check if JSON is empty as well
				Ok(response_json_raw) => {
					response_json = Some(response_json_raw);
					break;
				},
				Err(error) => {
					terminal_write(writer, format!("\nHTTP JSON 错误: {error}\n\n当前重试次数：{try_count}。\n无需担心，连续失败三次后会自动切换备用源！").as_str(), true, if writer_is_interactive { Some("red") } else { None });
					response_json = None;
					try_count += 1;

					// Try each server 3 times
					if try_count >= 3 {
						server_id += 1;
						try_count = 0;
					}
				}
			}
		} else {
			// get_http_response only returns None once every server is exhausted, so stop instead of looping forever
			break;
		}
	}

	response_json
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum IntegrityStatus {
	NeedDelete = 0,
	NeedOriginal = 1,
	NeedWipeFix = 2,
	NeedFix = 3,
	Fixed = 4
}

fn determine_file_integrity_status(gmod_path: PathBuf, filename: &str, hashes: &IndexMap<String, String>) -> Result<IntegrityStatus, String> {
	let file_parts: Vec<&str> = filename.split("/").collect();
	let file_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path, &file_parts[..]), false);
	let mut file_hash = BLANK_FILE_HASH.to_string();

	if let Ok(file_path) = file_path {
		file_hash = get_file_hash(&file_path)?;
	}

	if file_hash == hashes["fixed"] {
		Ok(IntegrityStatus::Fixed)
	} else {
		// File needs to be fixed...
		if hashes["fixed"] == BLANK_FILE_HASH {
			// This is a file that doesn't exist anymore after patching
			Ok(IntegrityStatus::NeedDelete)
		} else if hashes["original"] == BLANK_FILE_HASH {
			// The original file didn't exist, so we need to wipe/create the file, then patch it
			Ok(IntegrityStatus::NeedWipeFix)
		} else if file_hash == hashes["original"] {
			// The file is the original, so we just to apply the patch
			Ok(IntegrityStatus::NeedFix)
		} else {
			// We don't recognize the hash, so we need to first replace the file with the original (which we'll download), then apply the patch to that file
			Ok(IntegrityStatus::NeedOriginal)
		}
	}
}

async fn download_file_to_cache<W>(writer: fn() -> W, writer_is_interactive: bool, bar: ProgressBar, cache_dir: PathBuf, servers: &[&str], filename: String, target_hash: String, no_system_proxy: bool) -> Result<(), ()>
where
	W: std::io::Write + 'static
{
	let filename_no_zst = if filename.ends_with(".zst") {
		let len = filename.len() - 4;
		filename[..len].to_string()
	} else {
		filename.clone()
	};
	let file_parts: Vec<&str> = filename_no_zst.split("/").collect();
	let cache_file_path = extend_pathbuf_and_return(cache_dir, &file_parts[..]);
	let cache_file_path_result = path_to_canonical_pathbuf(&cache_file_path, false);

	terminal_write(writer, format!("\t正在下载: {filename} ...").as_str(), true, None);

	// Look in the cache to see if the file already exists
	if cache_file_path_result.is_ok() {
		let file_hash_result = get_file_hash(&cache_file_path);

		if let Ok(file_hash) = file_hash_result
			&& file_hash == target_hash {
				terminal_write(writer, format!("\t已从缓存加载: {filename}").as_str(), true, Some("green"));
				return Ok(());
			}
	}

	// If it's not in the cache, or there's a checksum mismatch with the version in the cache, (re-)download it
	let response_bytes = get_http_response_bytes(writer, writer_is_interactive, &bar, servers, filename.as_str(), no_system_proxy).await;
	if let Some(response_bytes) = response_bytes {
		// Create directories if needed
		let mut cache_file_path_dir = cache_file_path.clone();
		cache_file_path_dir.pop();
		let cache_file_path_dir_canonical = path_to_canonical_pathbuf(&cache_file_path_dir, false);

		if cache_file_path_dir_canonical.is_err() {
			let create_dir_result = tokio::fs::create_dir_all(cache_file_path_dir).await;

			if let Err(error) = create_dir_result {
				terminal_write(writer, format!("\t下载时失败: {filename} | 步骤 1: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return Err(());
			}
		}

		// Decompress Zstandard files
		let mut bytes: Vec<u8> = if filename.ends_with(".zst") { Vec::new() } else { response_bytes.to_vec() };
		if filename.ends_with(".zst") {
			terminal_write(writer, format!("\t正在解压: {filename} ...").as_str(), true, None);

			let decompress_result = zstd::stream::copy_decode(&response_bytes[..], &mut bytes);
			if let Err(error) = decompress_result {
				terminal_write(writer, format!("\t解压失败: {filename} | {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return Err(());
			}

			terminal_write(writer, format!("\t已解压: {filename}").as_str(), true, Some("green"));
		}

		let write_result = tokio::fs::write(cache_file_path.clone(), bytes).await;
		if let Err(error) = write_result {
			terminal_write(writer, format!("\t下载失败: {filename} | 步骤 2: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			return Err(());
		}

		let file_hash_result = get_file_hash(&cache_file_path);
		match file_hash_result {
			Ok(file_hash) => {
				if file_hash == target_hash {
					let size_mib = response_bytes.len() as f64 / 0x100000 as f64;
					terminal_write(writer, format!("\t已下载 [{size_mib:.2} MiB]: {filename}").as_str(), true, Some("green"));
					return Ok(());
				} else {
					terminal_write(writer, format!("\t下载失败: {filename} | 步骤 4: 校验未通过，可能是文件损坏").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				}
			},
			Err(error) => {
				terminal_write(writer, format!("\t下载失败: {filename} | 步骤 3: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			}
		}
	}

	Err(())
}

#[allow(clippy::too_many_arguments)]
fn patch_file<W>(
	writer: fn() -> W,
	writer_is_interactive: bool,
	integrity_status_strings: &HashMap<IntegrityStatus, &str>,
	gmod_path: &Path,
	platform_masked: &str,
	gmod_branch: &String,
	cache_dir: &Path,
	filename: &&String,
	integrity_status: &IntegrityStatus,
	hashes: &&IndexMap<String, String>
) -> IntegrityStatus
where
	W: std::io::Write + 'static
{
				terminal_write(writer, format!("\t正在修补: {filename} ...").as_str(), true, None);

	let mut new_integrity_status: IntegrityStatus = *integrity_status;
	let mut integrity_status_string = integrity_status_strings[&new_integrity_status];
	let gmod_file_parts: Vec<&str> = filename.split("/").collect();
	let gmod_file_path = extend_pathbuf_and_return(gmod_path.to_path_buf(), &gmod_file_parts[..]);

	// Delete the file since it's not used anymore
	// If we can't delete it outright, try and truncate it
	// We could alternatively "patch" it into being empty...but that's a waste of CPU cycles, and if truncating doesn't work, that won't work either
	if new_integrity_status == IntegrityStatus::NeedDelete {
		if let Err(delete_error) = std::fs::remove_file(&gmod_file_path)
			&& let Err(truncate_error) = File::create(&gmod_file_path) {
				terminal_write(writer, format!("\t修补时出现问题: {filename} | {integrity_status_string}:\n\t删除: {delete_error}\n\t截断: {truncate_error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}

		terminal_write(writer, format!("\t修补完成: {filename}").as_str(), true, Some("green"));
		new_integrity_status = IntegrityStatus::Fixed;
		integrity_status_string = integrity_status_strings[&new_integrity_status];
	}

	// Copy/overwrite the target gmod file with original copy we have
	if new_integrity_status == IntegrityStatus::NeedOriginal {
		let gmod_file_path_dir = gmod_file_path.parent().unwrap().to_path_buf();
		let gmod_file_path_dir_path = path_to_canonical_pathbuf(&gmod_file_path_dir, false);

		if gmod_file_path_dir_path.is_err() {
			let create_dir_result = std::fs::create_dir_all(gmod_file_path_dir);

			if let Err(error) = create_dir_result {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		}

		let original_filename = format!("originals/{platform_masked}/{gmod_branch}/{filename}");
		let original_file_parts: Vec<&str> = original_filename.split("/").collect();
		let original_cache_file_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(cache_dir.to_path_buf(), &original_file_parts[..]), false);

		match original_cache_file_path {
			Ok(original_cache_file_path) => {
				let copy_result = std::fs::copy(original_cache_file_path, &gmod_file_path);

				if let Err(error) = copy_result {
					terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
					return new_integrity_status;
				}

				new_integrity_status = IntegrityStatus::NeedFix;
				integrity_status_string = integrity_status_strings[&new_integrity_status];
			},
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		}
	}

	// Create/truncate original file (it doesn't exist without patches applied)
	if new_integrity_status == IntegrityStatus::NeedWipeFix {
		let gmod_file_path_dir = gmod_file_path.parent().unwrap().to_path_buf();
		let gmod_file_path_dir_path = path_to_canonical_pathbuf(&gmod_file_path_dir, false);

		if gmod_file_path_dir_path.is_err() {
			let create_dir_result = std::fs::create_dir_all(gmod_file_path_dir);

			if let Err(error) = create_dir_result {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		}

		let create_result = File::create(&gmod_file_path);

		if let Err(error) = create_result {
			terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			return new_integrity_status;
		}

		new_integrity_status = IntegrityStatus::NeedFix;
		integrity_status_string = integrity_status_strings[&new_integrity_status];
	}

	// Patch the original file into the fixed one!
	if new_integrity_status == IntegrityStatus::NeedFix {
		let gmod_file_path = match path_to_canonical_pathbuf(gmod_file_path, false) {
			Ok(gmod_file_path) => gmod_file_path,
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 1: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		};

		let patch_filename = format!("patches/{platform_masked}/{gmod_branch}/{filename}.bsdiff");
		let patch_file_parts: Vec<&str> = patch_filename.split("/").collect();

		let patch_file_path = match path_to_canonical_pathbuf(extend_pathbuf_and_return(cache_dir.to_path_buf(), &patch_file_parts[..]), false) {
			Ok(patch_file_path) => patch_file_path,
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 2: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		};

		let gmod_file = match std::fs::read(gmod_file_path.clone()) {
			Ok(gmod_file) => gmod_file,
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 3: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		};

		let patch_file = match std::fs::read(patch_file_path) {
			Ok(patch_file) => patch_file,
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 4: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		};

		let patcher = match Bspatch::new(&patch_file) {
			Ok(patcher) => patcher,
			Err(error) => {
				terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 5: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				return new_integrity_status;
			}
		};

		let mut new_gmod_file = Vec::with_capacity(patcher.hint_target_size() as usize);
		let patch_result = patcher.apply(&gmod_file, io::Cursor::new(&mut new_gmod_file));

		if let Err(error) = patch_result {
			terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 6: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			return new_integrity_status;
		}

		// Sanity check the checksum before writing, so a bad patch doesn't clobber the file on disk
		let file_hash = format!("{}", blake3::hash(&new_gmod_file));

		if file_hash != hashes["fixed"] {
			terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 7: 文件校验失败，可能是文件已损坏").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			return new_integrity_status;
		}

		let write_result = std::fs::write(&gmod_file_path, &new_gmod_file);

		if let Err(error) = write_result {
			terminal_write(writer, format!("\t修补时出错: {filename} | {integrity_status_string} / 步骤 8: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
			return new_integrity_status;
		}

		terminal_write(writer, format!("\t已修补: {filename}").as_str(), true, Some("green"));
		new_integrity_status = IntegrityStatus::Fixed;
	}

	new_integrity_status
}

#[cfg(unix)]
#[link(name = "c")]
unsafe extern "C" {
	safe fn geteuid() -> u32;
}

// HACK: Rip out the "WebStorage" section to mitigate stack overflow issues
// See `thread_stack_size` in main_script
// See https://github.com/CosmicHorrorDev/vdf-rs/issues/54
fn strip_localconfig_webstorage(localconfig: String) -> String {
	let webstorage_start_regex = Regex::new(r"WebStorage.+\s+\{").unwrap();
	let webstorage_start_match = webstorage_start_regex.find(&localconfig);

	if let Some(webstorage_start_match) = webstorage_start_match {
		let webstorage_open_bracket = webstorage_start_match.end();

		// Quoted values can contain unbalanced brackets (e.g. Overlay page titles), so only count brackets outside strings
		let mut next_char_escaped = false;
		let mut in_string = false;
		let mut open_bracket_count: usize = 1;
		let mut webstorage_close_bracket_offset: Option<usize> = None;
		for (offset, byte) in (0_usize..).zip(localconfig[webstorage_open_bracket..].bytes()) {
			if next_char_escaped {
				next_char_escaped = false;
			} else if byte == b'\\' {
				next_char_escaped = true;
			} else if byte == b'"' {
				in_string = !in_string;
			} else if !in_string {
				if byte == b'{' {
					open_bracket_count += 1;
				} else if byte == b'}' {
					open_bracket_count -= 1;
				}

				if open_bracket_count == 0 {
					webstorage_close_bracket_offset = Some(offset);
					break;
				}
			}
		}

		if let Some(webstorage_close_bracket_offset) = webstorage_close_bracket_offset {
			let webstorage_close_bracket = webstorage_open_bracket + webstorage_close_bracket_offset;
			return format!("{}{}", &localconfig[..webstorage_open_bracket], &localconfig[webstorage_close_bracket..]);
		}
	}

	localconfig
}

async fn main_script_internal<W>(writer: fn() -> W, writer_is_interactive: bool, args: Args) -> Result<(), AlmightyError>
where
	W: std::io::Write + 'static
{
	let now = Instant::now();
	let mut sys = System::new_all();

	// Set up the live progress UI; hidden when output isn't a terminal so we don't spam control codes
	let _ = UI.set(MultiProgress::with_draw_target(if writer_is_interactive {
		ProgressDrawTarget::stdout()
	} else {
		ProgressDrawTarget::hidden()
	}));

	// Figure out where our cache should go based on OS
	let os_cache_dir = if let Some(dirs_cache_dir) = dirs::cache_dir() { dirs_cache_dir } else { std::env::temp_dir() };

	// The cache base (ex. ~/.cache) may not exist on a fresh account; the lockfile create reports any real failure
	let _ = std::fs::create_dir_all(&os_cache_dir);

	// Take the PID lockfile atomically (O_EXCL); a pre-existing file only counts as running if it points at a live gmodpatchtool
	let pid_path = extend_pathbuf_and_return(os_cache_dir.clone(), &["gmodpatchtool.pid"]);
	let mut lock_attempts = 0;
	loop {
		match std::fs::OpenOptions::new().write(true).create_new(true).open(&pid_path) {
			Ok(mut pid_file) => {
				use std::io::Write;
				if let Err(error) = pid_file.write_all(std::process::id().to_string().as_bytes()) {
					return Err(AlmightyError::Generic(format!("创建 gmodpatchtool.pid 时失败: {error}")));
				}
				break;
			},
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists && lock_attempts < 2 => {
				lock_attempts += 1;

				let existing_pid = std::fs::read_to_string(&pid_path).ok().and_then(|pid| pid.trim().parse::<usize>().ok());
				if let Some(existing_pid) = existing_pid
					&& let Some(process) = sys.process(sysinfo::Pid::from(existing_pid))
						&& process.name().to_string_lossy().starts_with("gmodpatchtool") {
							return Err(AlmightyError::Generic(format!("已有 GModPatchTool 正在运行（进程 PID: {existing_pid}）！")));
						}

				// Stale lockfile (crashed run or recycled PID); remove it and retry
				let _ = std::fs::remove_file(&pid_path);
			},
			Err(error) => {
				return Err(AlmightyError::Generic(format!("创建 gmodpatchtool.pid 时失败: {error}")));
			}
		}
	}

	// 启动时自动检测镜像源下载速度：优先 gh-proxy（IPv4 优选节点），速度不理想则退回 scs.mf.nekogan.com
	let mirror_selection = select_mirror_sources(writer, writer_is_interactive, args.no_system_proxy).await;

	// Get local version
	let local_version: u32 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();

	// Get remote version
	terminal_write(writer, "正在获取最新版本信息...\n", true, None);

	let remote_version = get_http_response_text(writer, writer_is_interactive, &mirror_selection.text_roots, "version.txt", args.no_system_proxy).await;

	if remote_version.is_none() {
		return Err(AlmightyError::Generic("获取最新版本时出错，请检查你的网络连接".to_string()));
	}

	let remote_version = remote_version.unwrap();
	let remote_version: u32 = remote_version
	.trim()
	.parse()?;

	if local_version >= remote_version {
		terminal_write(writer, format!("🎉恭喜，你正在运行最新版的 GModPatchTool\n[当前版本: {local_version} / 最新版本: {remote_version}]!\n").as_str(), true, if writer_is_interactive { Some("green") } else { None });
	} else {
		terminal_write(writer, "⚠️警告: GModPatchTool 有新版本啦！建议下载最新版本以获取更多新内容！\nhttps://juice.catkim.xyz/index.php/gmodcefcodecfix/", true, if writer_is_interactive { Some("red") } else { None });

		let mut secs_to_continue: u8 = 5;
		while secs_to_continue > 0 {
			terminal_write(writer, format!("\t将在 {secs_to_continue} 秒后继续...\r").as_str(), false, if writer_is_interactive { Some("yellow") } else { None });
			writer().flush().unwrap();
			tokio::time::sleep(time::Duration::from_secs(1)).await;
			secs_to_continue -= 1;
		}

		// Clear continuing line
		if writer_is_interactive {
			terminal_write(writer, "\x1B[0K\n", false, None);
		}
	}

	// Warn/Exit if running as root/admin
	#[cfg(windows)]
	let root = is_elevated();

	#[cfg(unix)]
	let root = geteuid() == 0;

	if root {
		if args.run_as_root_with_security_risk {
			terminal_write(writer, "⚠️警告: 你当前正以管理员身份运行 GModPatchTool！这是没有必要的，而且会导致一些不该出现的问题！", true, if writer_is_interactive { Some("red") } else { None });
	terminal_write(writer, "（镜像版吐槽：Python 时代的 GCCF 怎么没有问题？）", true, None);

			let mut secs_to_continue: u8 = 10;
			while secs_to_continue > 0 {
				terminal_write(writer, format!("\t此条警告会继续显示 {secs_to_continue} 秒...\r").as_str(), false, if writer_is_interactive { Some("yellow") } else { None });
				writer().flush().unwrap();
				tokio::time::sleep(time::Duration::from_secs(1)).await;
				secs_to_continue -= 1;
			}

			// Clear continuing line
			if writer_is_interactive {
				terminal_write(writer, "\x1B[0K\n", false, None);
			}
		} else {
			terminal_write(writer, "⚠️警告: 你当前正以管理员身份运行 GModPatchTool！这是没有必要的，而且会导致一些不该出现的问题！", true, if writer_is_interactive { Some("red") } else { None });

			if cfg!(windows) {
				terminal_write(writer, "（如果你确定没有使用管理员权限运行，那么 UAC 账户控制可能已被关闭！）", true, None);
			}

			terminal_write(writer, "是否仍要继续修补？输入 y 或直接按回车继续；输入 n 退出。", true, if writer_is_interactive { Some("yellow") } else { None });
			writer().flush().unwrap();

			let mut answer = String::new();
			// 无可交互输入（脚本管道等）时默认继续，避免无法应答被卡死
			let confirmed = match std::io::stdin().read_line(&mut answer) {
				Ok(0) | Err(_) => true,
				Ok(_) => !matches!(answer.trim().to_lowercase().as_str(), "n" | "no" | "否" | "不")
			};

			if !confirmed {
				return Err(AlmightyError::Generic("已按你的选择退出程序。下次运行如需跳过此询问，可添加启动参数 --run-as-root-with-security-risk。".to_string()));
			}

			terminal_write(writer, "", true, None);
		}
	}

	// Abort if GMod is currently running
	if !args.ignore_gmod_running && (sys.processes_by_exact_name("gmod.exe".as_ref()).next().is_some() || sys.processes_by_exact_name("gmod".as_ref()).next().is_some()) {
		return Err(AlmightyError::Generic("😭 Garry's Mod 已在运行，请先关闭它！".to_string()));
	}

	// Warning for macOS users
	#[cfg(target_os = "macos")]
	{
		terminal_write(writer, "警告: Garry's Mod 对 macOS 的支持并不好，并且以后也没有对 macOS 的支持进行优化的计划：", true, if writer_is_interactive { Some("red") } else { None });
		terminal_write(writer, "\thttps://winteris.moe/share/2025-08-07_20-53-45.png", true, None);
		terminal_write(writer, "\n我们推荐你在 macOS 上游玩 Windows 版本的 GMod，详情请看:", true, if writer_is_interactive { Some("red") } else { None });
		terminal_write(writer, "\thttps://github.com/solsticegamestudios/GModPatchTool/issues/170", true, None);

		let mut secs_to_continue: u8 = 10;
		while secs_to_continue > 0 {
			terminal_write(writer, format!("\t将在 {secs_to_continue} 秒后继续...\r").as_str(), false, if writer_is_interactive { Some("yellow") } else { None });
			writer().flush().unwrap();
			tokio::time::sleep(time::Duration::from_secs(1)).await;
			secs_to_continue -= 1;
		}

		// Clear continuing line
		if writer_is_interactive {
			terminal_write(writer, "\x1B[0K\n", false, None);
		}
	}

	// Find Steam
	// None is the Windows/Linux fallback when no path is found; macOS always reassigns
	#[allow(unused_assignments)]
	let mut steam_path = None;
	if let Some(steam_path_arg) = args.steam_path {
		// Make sure the path the user is forcing actually exists
		let steam_path_arg_pathbuf = path_to_canonical_pathbuf(&steam_path_arg, true);

		steam_path = match steam_path_arg_pathbuf {
			Ok(steam_path) => Some(steam_path),
			Err(error) => {
				return Err(AlmightyError::Generic(format!("请检查 --steam-path 参数是否指向有效路径:\n\t{error}")));
			}
		}
	} else {
		// Windows
		#[cfg(windows)]
		{
			if let Ok(steam_reg_key) = windows_registry::CURRENT_USER.open("Software\\Valve\\Steam") {
				if let Ok(steam_reg_path) = steam_reg_key.get_string("SteamPath") {
					steam_path = path_to_canonical_pathbuf(steam_reg_path, true).ok();
				}
			}
		}

		// macOS
		#[cfg(target_os = "macos")]
		{
			// $HOME/Library/Application Support/Steam
			let mut steam_data_path = dirs::data_dir().unwrap();
			steam_data_path.push("Steam");
			steam_path = path_to_canonical_pathbuf(steam_data_path, true).ok();
		}

		// Anything else (we assume Linux)
		#[cfg(not(any(windows, target_os = "macos")))]
		{
			let home_dir = dirs::home_dir().unwrap();
			let possible_steam_paths = vec![
				// Snap
				extend_pathbuf_and_return(home_dir.clone(), &["snap", "steam", "common", ".local", "share", "Steam"]),
				extend_pathbuf_and_return(home_dir.clone(), &["snap", "steam", "common", ".steam", "steam"]),
				// Flatpak
				extend_pathbuf_and_return(home_dir.clone(), &[".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"]),
				extend_pathbuf_and_return(home_dir.clone(), &[".var", "app", "com.valvesoftware.Steam", ".steam", "steam"]),
				// Home
				extend_pathbuf_and_return(home_dir.clone(), &[".steam", "steam"]),
				//extend_pathbuf_and_return(home_dir.clone(), &[".steam"]),
			];
			let mut valid_steam_paths = vec![];

			for pathbuf in possible_steam_paths {
				if let Ok(pathbuf) = path_to_canonical_pathbuf(pathbuf, true) {
					if !valid_steam_paths.contains(&pathbuf) {
						valid_steam_paths.push(pathbuf);
					}
				}
			}

			// $XDG_DATA_HOME/Steam
			if let Some(steam_xdg_path) = dirs::data_dir() {
				if let Ok(steam_xdg_pathbuf) = path_to_canonical_pathbuf(extend_pathbuf_and_return(steam_xdg_path, &["Steam"]), true) {
					if !valid_steam_paths.contains(&steam_xdg_pathbuf) {
						valid_steam_paths.push(steam_xdg_pathbuf);
					}
				}
			}

			// Set the Steam path if at least one is valid
			// Warn if there's more than one
			if !valid_steam_paths.is_empty() {
				if valid_steam_paths.len() > 1 {
					let mut valid_steam_paths_str: String = "".to_string();
					for pathbuf in &valid_steam_paths {
						valid_steam_paths_str += "\n\t- ";
						valid_steam_paths_str += &pathbuf.to_string_lossy();
					}

					terminal_write(writer, format!("⚠️警告: 检测到多个 Steam 安装路径！这可能会导致一些问题：{valid_steam_paths_str}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });

					let mut secs_to_continue: u8 = 5;
					while secs_to_continue > 0 {
						terminal_write(writer, format!("\t该警告还会继续显示 {secs_to_continue} 秒...\r").as_str(), false, if writer_is_interactive { Some("yellow") } else { None });
						writer().flush().unwrap();
						tokio::time::sleep(time::Duration::from_secs(1)).await;
						secs_to_continue -= 1;
					}

					// Clear continuing line
					if writer_is_interactive {
						terminal_write(writer, "\x1B[0K\n", false, None);
					}
				}

				steam_path = Some(valid_steam_paths[0].clone());
			}
		}
	}

	if steam_path.is_none() {
		return Err(AlmightyError::Generic("❌未找到 Steam 安装路径！如果你确定已经安装，可以添加参数 --steam-path 手动指定 Steam 的安装路径。".to_string()));
	}

	let steam_path = steam_path.unwrap();
	let steam_path_str = steam_path.to_string_lossy();

	terminal_write(writer, format!("🎉Steam 路径: {steam_path_str}\n").as_str(), true, None);

	// Get most recent Steam User, which is probably the one they're using/want
	let steam_loginusers_path = extend_pathbuf_and_return(steam_path.clone(), &["config", "loginusers.vdf"]);
	let steam_loginusers_bytes = tokio::fs::read(steam_loginusers_path).await;

	if steam_loginusers_bytes.is_err() {
		return Err(AlmightyError::Generic("❌loginusers.vdf 去异世界旅游了！它可能从未存在过，也可能只是走丢了。可以尝试重新登录 Steam 把它召回来，或者摆个法阵召唤一下。".to_string()));
	}

	// loginusers.vdf can hold invalid UTF-8 (truncated persona names), so decode lossily
	let steam_loginusers_str = String::from_utf8_lossy(&steam_loginusers_bytes.unwrap()).into_owned();
	let steam_loginusers = vdf::from_str(steam_loginusers_str.as_str());

	if let Err(error) = steam_loginusers {
		return Err(AlmightyError::Generic(format!("❌loginusers.vdf 发生了血肉畸变，我们已经不认识它了！\n\t{error}")));
	}

	let mut steam_user: HashMap<&str, String> = HashMap::new();
	let mut steam_user_is_most_recent = false;
	let steam_loginusers: HashMap<&str, SteamUser> = steam_loginusers.unwrap();
	for (other_steam_id_64, other_steam_user) in steam_loginusers {
		let mostrecent = other_steam_user.most_recent;
		let timestamp = other_steam_user.timestamp;

		// MostRecent wins outright; otherwise the newest timestamp wins, but never over a MostRecent pick
		let take = !steam_user.contains_key("Timestamp")
			|| mostrecent
			|| (!steam_user_is_most_recent && timestamp > steam_user.get("Timestamp").unwrap().parse::<u64>().unwrap());

		if take {
			steam_user_is_most_recent = mostrecent;
			steam_user.insert("SteamID64", other_steam_id_64.to_string());
			steam_user.insert("Timestamp", timestamp.to_string());
			steam_user.insert("AccountName", other_steam_user.account_name);
			steam_user.insert("PersonaName", other_steam_user.persona_name);
		}
	}

	if !steam_user.contains_key("Timestamp") {
		return Err(AlmightyError::Generic("❌我的高达驾驶员呢？你有登录过 Steam 吗？".to_string()));
	}

	let steam_id = steam_user.get("SteamID64").unwrap().parse::<u64>().ok().and_then(|steam_id_64| SteamId::new(steam_id_64).ok());
	let Some(steam_id) = steam_id else {
		return Err(AlmightyError::Generic("❌loginusers.vdf 发生了血肉畸变，我们已经不认识它了！".to_string()));
	};

	terminal_write(writer, format!("Steam User: {} ({} / {})\n", steam_user.get("PersonaName").unwrap(), steam_user.get("SteamID64").unwrap(), steam_id.steam3id()).as_str(), true, None);

	// Get Steam Libraries
	let mut steam_libraryfolders_path = extend_pathbuf_and_return(steam_path.clone(), &["config", "libraryfolders.vdf"]);
	let mut steam_libraryfolders_str = tokio::fs::read_to_string(steam_libraryfolders_path).await;

	// Try steamapps
	if steam_libraryfolders_str.is_err() {
		steam_libraryfolders_path = extend_pathbuf_and_return(steam_path.clone(), &["steamapps", "libraryfolders.vdf"]);
		steam_libraryfolders_str = tokio::fs::read_to_string(steam_libraryfolders_path).await;
	}

	// Try SteamApps with capitalization
	if steam_libraryfolders_str.is_err() {
		steam_libraryfolders_path = extend_pathbuf_and_return(steam_path.clone(), &["SteamApps", "libraryfolders.vdf"]);
		steam_libraryfolders_str = tokio::fs::read_to_string(steam_libraryfolders_path).await;
	}

	if steam_libraryfolders_str.is_err() {
		return Err(AlmightyError::Generic("❌libraryfolders.vdf 去异世界旅游了！它可能从未存在过，也可能只是走丢了。可以尝试重新登录 Steam 把它召回来，或者摆个法阵召唤一下。".to_string()));
	}

	let steam_libraryfolders_str = steam_libraryfolders_str.unwrap();
	let steam_libraryfolders = vdf::from_str(steam_libraryfolders_str.as_str());

	if let Err(error) = steam_libraryfolders {
		return Err(AlmightyError::Generic(format!("❌libraryfolders.vdf 发生了血肉畸变，我们已经不认识它了！\n\t{error}")));
	}

	// Get GMod Steam Library and Manifest
	let mut gmod_steam_library_path = None;
	let mut gmod_manifest_str = None;

	// IndexMap keeps the VDF's order so a stale appmanifest in a later library can't randomly win over the real install
	let steam_libraryfolders: IndexMap<&str, SteamLibraryFolderEntry> = steam_libraryfolders.unwrap();
	for (_, steam_library) in steam_libraryfolders {
		// Skip non-folder entries (number values like contentstatsid)
		let SteamLibraryFolderEntry::Folder(steam_library) = steam_library else {
			continue;
		};

		// Get potential Steam Library
		let new_gmod_steam_library_path = path_to_canonical_pathbuf(steam_library.path, true);

		if let Ok(new_gmod_steam_library_path) = new_gmod_steam_library_path {
			// Get GMod manifest
			let mut new_gmod_manifest_path = extend_pathbuf_and_return(new_gmod_steam_library_path.to_path_buf(), &["steamapps", "appmanifest_4000.acf"]);
			let mut new_gmod_manifest_str = tokio::fs::read_to_string(new_gmod_manifest_path).await;

			// Try SteamApps with capitalization
			if new_gmod_manifest_str.is_err() {
				new_gmod_manifest_path = extend_pathbuf_and_return(new_gmod_steam_library_path.to_path_buf(), &["SteamApps", "appmanifest_4000.acf"]);
				new_gmod_manifest_str = tokio::fs::read_to_string(new_gmod_manifest_path).await;
			}

			if new_gmod_manifest_str.is_ok() {
				gmod_steam_library_path = Some(new_gmod_steam_library_path);
				gmod_manifest_str = new_gmod_manifest_str.ok();
				break;
			}
		}
	}

	//gmod_steam_library_path.is_none() ||
	if gmod_manifest_str.is_none() {
		return Err(AlmightyError::Generic("❌未找到 appmanifest_4000.acf！\n😭啊，GMod 你怎么似了！".to_string()));
	}

	let gmod_manifest_str = gmod_manifest_str.unwrap();
	let gmod_manifest = vdf::from_str(gmod_manifest_str.as_str());

	if let Err(error) = gmod_manifest {
		return Err(AlmightyError::Generic(format!("❌无法解析 appmanifest_4000.acf！\n😭啊，GMod 你怎么似了！\n\t{error}")));
	}

	let gmod_steam_library_path = gmod_steam_library_path.unwrap();
	let gmod_steam_library_path_str = gmod_steam_library_path.to_string_lossy();

	terminal_write(writer, format!("🎉GMod Steam 安装位置: {gmod_steam_library_path_str}\n").as_str(), true, None);

	// Get GMod app state
	let gmod_manifest: SteamAppManifest = gmod_manifest.unwrap();
	let gmod_stateflags = gmod_manifest.state_flags;
	//let gmod_downloadtype = gmod_manifest.download_type; // TODO: Figure this out...
	let gmod_scheduledautoupdate = gmod_manifest.scheduled_auto_update;
	let gmod_fullvalidatebeforenextupdate: bool = gmod_manifest.full_validate_before_next_update.unwrap_or_default();
	let gmod_bytesdownloaded = gmod_manifest.bytes_downloaded;
	let gmod_bytestodownload = gmod_manifest.bytes_to_download;
	let gmod_bytesstaged = gmod_manifest.bytes_staged;
	let gmod_bytestostage = gmod_manifest.bytes_to_stage;

	terminal_write(writer, format!("GMod 应用状态: {gmod_stateflags} | {gmod_scheduledautoupdate} | {gmod_fullvalidatebeforenextupdate} | {gmod_bytesdownloaded}/{gmod_bytestodownload} | {gmod_bytesstaged}/{gmod_bytestostage} \n").as_str(), true, None);

	if gmod_stateflags != 4 || gmod_scheduledautoupdate != 0 || gmod_fullvalidatebeforenextupdate || gmod_bytesdownloaded != gmod_bytestodownload || gmod_bytesstaged != gmod_bytestostage {
		return Err(AlmightyError::Generic("🤔看起来 Garry's Mod 还没有准备好！请在 Steam > 下载 中确认 GMod 不在更新队列中！❓如果仍然无效，可以试试把游戏打开再关闭！".to_string()));
	}

	// Get GMod branch
	// TODO: Change branch to x86-64 if the current branch isn't in the manifest
	let gmod_mountedconfig = gmod_manifest.mounted_config;
	let gmod_branch = gmod_mountedconfig.beta_key;
	let gmod_branch = match gmod_branch { Some(gmod_branch) if !gmod_branch.is_empty() => gmod_branch, _ => "public".to_string() };

	terminal_write(writer, format!("GMod 测试渠道: {gmod_branch}\n").as_str(), true, None);

	// Get GMod path
	// TODO: What about `steamapps/<username>/GarrysMod`? Is that still a thing, or did SteamPipe kill/migrate it completely?
	let gmod_path_config = gmod_manifest.install_dir;
	let mut gmod_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_steam_library_path.clone(), &["steamapps", "common", &gmod_path_config]), true);

	// Try SteamApps with capitalization
	if gmod_path.is_err() {
		gmod_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_steam_library_path.clone(), &["SteamApps", "common", &gmod_path_config]), true);
	}

	if gmod_path.is_err() {
		return Err(AlmightyError::Generic("未找到 Garry's Mod 的安装目录！你确定安装过 GMod 吗？".to_string()));
	}

	let gmod_path = gmod_path.unwrap();
	let gmod_path_str = gmod_path.to_string_lossy();

	terminal_write(writer, format!("GMod 路径: {gmod_path_str}\n").as_str(), true, None);

	// Abort if they're running as root AND the GMod directory isn't owned by root
	// Will hopefully prevent broken installs/updating
	#[cfg(unix)]
	if root
		&& let Ok(gmod_dir_meta) = tokio::fs::metadata(&gmod_path).await
			&& gmod_dir_meta.uid() != 0 {
				return Err(AlmightyError::Generic("你正以 root 身份运行 GModPatchTool，并成功触发了之前的警告！Garry's Mod 安装目录的所有者不是 root，请重新分配目录权限，或改用其他账户运行本程序！\n程序即将退出...".to_string()));
			}

	// Determine target platform
	// Get GMod CompatTool config (Steam Linux Runtime, Proton, etc) on Linux
	// NOTE: platform_masked is specifically for Proton
	let platform = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };

	#[cfg_attr(not(target_os = "linux"), expect(unused_mut, reason = "used on linux"))]
	let mut platform_masked = platform;

	#[cfg_attr(not(target_os = "linux"), expect(unused_mut, reason = "used on linux"))]
	let mut gmod_compattool = "none".to_string();

	#[cfg(target_os = "linux")]
	{
		// Get Steam config
		let steam_config_path = extend_pathbuf_and_return(steam_path.clone(), &["config", "config.vdf"]);
		let steam_config_str = tokio::fs::read_to_string(steam_config_path).await;

		if steam_config_str.is_err() {
		return Err(AlmightyError::Generic("❌config.vdf 去异世界旅游了！它可能从未存在过，也可能只是走丢了。可以尝试重新登录 Steam 把它召回来，或者摆个法阵召唤一下。".to_string()));
		}

		let steam_config_str = steam_config_str.unwrap();
		let steam_config = vdf::from_str(steam_config_str.as_str());

		if steam_config.is_err() {
			return Err(AlmightyError::Generic("❌config.vdf 发生了血肉畸变，我们已经不认识它了！".to_string()));
		}

		let steam_config: SteamConfig = steam_config.unwrap();
		let steam_config = steam_config.software.valve.steam;

		if let Some(steam_config_compat_tool_mapping) = steam_config.compat_tool_mapping {
			if let Some(steam_config_compat_tool_mapping_gmod) = steam_config_compat_tool_mapping.gmod {
				let compattool = steam_config_compat_tool_mapping_gmod.name.to_lowercase();

				if compattool.contains("proton") {
					platform_masked = "windows";
				}

				gmod_compattool = compattool;
			}
		}
	}

	terminal_write(writer, format!("目标平台: {platform_masked} ({gmod_compattool})\n").as_str(), true, None);

	// Warn if -nochromium is in launch options
	// Some GMod "menu error fix" guides include it + gmod-lua-menu
	let steam_user_localconfig_path = extend_pathbuf_and_return(steam_path.clone(), &["userdata", steam_id.account_id().into_u32().to_string().as_str(), "config", "localconfig.vdf"]);
	let steam_user_localconfig_bytes = tokio::fs::read(steam_user_localconfig_path).await;

	if let Err(error) = steam_user_localconfig_bytes {
		return Err(AlmightyError::Generic(format!("❌localconfig.vdf 去异世界旅游了！它可能从未存在过，也可能只是走丢了。可以尝试重新登录 Steam 把它召回来，或者摆个法阵召唤一下。\n\t{error}")));
	}

	// localconfig.vdf can hold invalid UTF-8 (Steam truncates friend persona names mid-codepoint), so decode lossily
	let steam_user_localconfig_str = strip_localconfig_webstorage(String::from_utf8_lossy(&steam_user_localconfig_bytes.unwrap()).into_owned());

	let steam_user_localconfig = vdf::from_str(steam_user_localconfig_str.as_str());

	if let Err(error) = steam_user_localconfig {
		return Err(AlmightyError::Generic(format!("❌localconfig.vdf 发生了血肉畸变，我们已经不认识它了！\n\t{error}")));
	}

	let steam_user_localconfig: SteamUserLocalConfig = steam_user_localconfig.unwrap();
	let steam_user_localconfig_gmod = steam_user_localconfig.software.valve.steam.apps.gmod;

	if let Some(steam_user_localconfig_gmod) = steam_user_localconfig_gmod {
		if let Some(steam_user_localconfig_gmod_launchopts) = &steam_user_localconfig_gmod.launch_options
			&& steam_user_localconfig_gmod_launchopts.contains("-nochromium") {
				terminal_write(writer, "⚠️警告: 检测到 GMod 启动参数 -nochromium！CEF 与它水火不容！\n\t请在 Steam > Garry's Mod > 属性 > 通用 中移除该参数！\n\t此外，如果安装了 gmod-lua-menu，请一并卸载。", true, if writer_is_interactive { Some("yellow") } else { None });

				let mut secs_to_continue: u8 = 5;
				while secs_to_continue > 0 {
					terminal_write(writer, format!("\t该警告还会继续显示 {secs_to_continue} 秒...\r").as_str(), false, if writer_is_interactive { Some("yellow") } else { None });
					writer().flush().unwrap();
					tokio::time::sleep(time::Duration::from_secs(1)).await;
					secs_to_continue -= 1;
				}

				// Clear continuing line
				if writer_is_interactive {
					terminal_write(writer, "\x1B[0K\n", false, None);
				}
			}
	} else {
		return Err(AlmightyError::Generic("Couldn't find Garry's Mod in user localconfig.vdf. Is Garry's Mod installed?".to_string()));
	}

	// Get remote manifest
	terminal_write(writer, "获取远程文件中...", true, None);

	let remote_manifest = get_http_response_json::<_, Manifest>(writer, writer_is_interactive, &mirror_selection.text_roots, "manifest.json", args.no_system_proxy).await;

	if remote_manifest.is_none() {
		terminal_write(writer, "", true, None); // Newline
		return Err(AlmightyError::Generic("无法获取远程文件列表！请检查你的网络连接".to_string()));
	}

	let remote_manifest = remote_manifest.unwrap();

	terminal_write(writer, "GModPatchTool 文件列表已载入！\n", true, None);

	let platform_branches = remote_manifest.get(platform_masked);
	if platform_branches.is_none() {
		return Err(AlmightyError::Generic(format!("你的操作系统 ({platform_masked}) 不受支持！")));
	}

	let platform_branch_files = platform_branches.unwrap().get(&gmod_branch);
	if platform_branch_files.is_none() {
		return Err(AlmightyError::Generic(format!("喵喵亲，请先把你的 GMod 测试版设置为 x86_64！当前测试版 ({gmod_branch}) 不受支持！")));
	}

	let platform_branch_files = platform_branch_files.unwrap();

	// Reject manifest paths that could escape the GMod directory or inject terminal escapes, in case the manifest is ever compromised
	for filename in platform_branch_files.keys() {
		let path_is_safe = std::path::Path::new(filename).components().all(|component| matches!(component, std::path::Component::Normal(_)));
		if !path_is_safe || filename.chars().any(|character| character.is_control()) {
			return Err(AlmightyError::Generic(format!("文件列表包含不安全路径: {filename:?}")));
		}
	}

	// Determine file integrity status
	terminal_write(writer, "正在确定文件完整性状态...", true, None);

	// TODO: phf_map for these
	let integrity_status_strings = HashMap::from([
		(IntegrityStatus::NeedDelete, "待删除"),
		(IntegrityStatus::NeedOriginal, "待修补(OF)"),
		(IntegrityStatus::NeedWipeFix, "待修补(WF)"),
		(IntegrityStatus::NeedFix, "待修补(F)"),
		(IntegrityStatus::Fixed, "已修补")
	]);

	#[allow(clippy::type_complexity)]
	let integrity_results: Vec<(&String, Result<IntegrityStatus, String>, &IndexMap<String, String>)> = platform_branch_files.par_iter()
	.map(|(filename, hashes)| {
		let integrity_result;
		if args.no_sourcescheme && filename.ends_with(".res") {
			terminal_write(writer, format!("\t{filename}: 因 --no-sourcescheme 而被跳过").as_str(), true, if writer_is_interactive { Some("green") } else { None });
			integrity_result = Ok(IntegrityStatus::Fixed);
		} else {
			integrity_result = determine_file_integrity_status(gmod_path.clone(), filename, hashes);
			let integrity_result_clone = integrity_result.clone();

			match integrity_result_clone {
				Ok(integrity_result_clone) => {
					let integrity_status_string = integrity_status_strings[&integrity_result_clone];
					terminal_write(writer, format!("\t{filename}: {integrity_status_string}").as_str(), true, if integrity_result_clone == IntegrityStatus::Fixed { Some("green") } else { None });
				},
				Err(error) => {
					terminal_write(writer, format!("\t{filename}: {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
				}
			}
		}

		(filename, integrity_result, hashes)
	}).collect();

	// Filter out fixed files, and if there were any i/o errors getting the hash, exit early
	// We don't exit during the multithreaded iterator above because we want *all* of the failing files to list first
	let mut pending_files: Vec<(&String, IntegrityStatus, &IndexMap<String, String>)> = vec![];
	for (filename, result, hashes) in integrity_results {
		match result {
			Ok(result) => {
				if result != IntegrityStatus::Fixed {
					pending_files.push((filename, result, hashes));
				}
			},
			Err(_) => {
				return Err(AlmightyError::Generic("无法获取一个或多个文件的完整性状态！".to_string()));
			}
		}
	}

	let pending_files_len = pending_files.len();
	if pending_files_len > 0 {
		// Delete old GModCEFCodecFix cache directory
		#[cfg(windows)]
		let old_cache_dir = path_to_canonical_pathbuf(extend_pathbuf_and_return(os_cache_dir.clone(), &["Temp", "GModCEFCodecFix"]), false);

		#[cfg(not(windows))]
		let old_cache_dir = path_to_canonical_pathbuf(extend_pathbuf_and_return(os_cache_dir.clone(), &["GModCEFCodecFix"]), false);

		if let Ok(old_cache_dir) = old_cache_dir {
			let old_cache_dir_result = tokio::fs::remove_dir_all(old_cache_dir).await;

			match old_cache_dir_result {
				Ok(_) => {
					terminal_write(writer,"已成功删除旧的 GModCEFCodecFix 缓存目录", true, None);
				},
				Err(error) => {
					terminal_write(writer, format!("无法删除旧的 GModCEFCodecFix 缓存目录：{error}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
				}
			}
		}

		// Create new GModPatchTool cache directory if it doesn't exist
		let cache_path = extend_pathbuf_and_return(os_cache_dir, &["GModPatchTool"]);
		let mut cache_path_str = cache_path.to_string_lossy();
		let mut cache_dir = path_to_canonical_pathbuf(&cache_path, false);

		// ...but make sure it doesn't exist (and clear it) if disable_cache is set
		if args.disable_cache {
			if let Ok(cache_dir) = cache_dir {
				let remove_result = tokio::fs::remove_dir_all(cache_dir).await;

				match remove_result {
					Ok(_) => {
						terminal_write(writer,"\n[disable-cache:Pre] 已成功清除 GModPatchTool 缓存目录", true, None);
					},
					Err(error) => {
						terminal_write(writer, format!("\n[disable-cache:Pre] 清除 GModPatchTool 缓存目录失败：{error}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
					}
				}
			}

			cache_dir = path_to_canonical_pathbuf(&cache_path, false);
		}

		if cache_dir.is_err() {
			let create_result = tokio::fs::create_dir_all(cache_path.clone()).await;

			if create_result.is_ok() {
				cache_dir = path_to_canonical_pathbuf(&cache_path, false);
			}
		}

		// Can't access or create the cache directory!
		if let Err(error) = cache_dir {
			return Err(AlmightyError::Generic(format!("创建缓存目录失败 ({error}):\n\t{cache_path_str}")));
		}

		let cache_dir = cache_dir.unwrap();
		cache_path_str = cache_dir.to_string_lossy();

		terminal_write(writer, format!("\nGModPatchTool 缓存目录: {cache_path_str}\n").as_str(), true, None);

		// Download what we need
		terminal_write(writer, "正在下载文件...", true, None);

		// Length starts at 1 (0 renders as a full bar) and grows as each download's Content-Length arrives
		let download_bar = UI.get().unwrap().add(ProgressBar::new(1));
		download_bar.set_style(spinner_style("{spinner} 正在下载 {msg} 个文件 [{bar:25}] {percent}% \u{B7} {bytes}/{total_bytes} ({binary_bytes_per_sec})"));
		download_bar.enable_steady_tick(SPINNER_TICK);

		let mut download_futures = JoinSet::new();
		for (filename, integrity_status, hashes) in &pending_files {
			// Need Original
			if *integrity_status == IntegrityStatus::NeedOriginal {
				download_futures.spawn(download_file_to_cache(writer, writer_is_interactive, download_bar.clone(), cache_dir.clone(), &mirror_selection.binary_roots, format!("originals/{platform_masked}/{gmod_branch}/{filename}.zst"), hashes["original"].clone(), args.no_system_proxy));
			}

			// Need Fix (we filtered out IntegrityStatus::Fixed above, but we still need IntegrityStatus::NeedDelete for later)
			if *integrity_status != IntegrityStatus::NeedDelete {
				download_futures.spawn(download_file_to_cache(writer, writer_is_interactive, download_bar.clone(), cache_dir.clone(), &mirror_selection.binary_roots, format!("patches/{platform_masked}/{gmod_branch}/{filename}.bsdiff"), hashes["patch"].clone(), args.no_system_proxy));
			}
		}

		let download_files_total = download_futures.len();
		let mut download_files_done: usize = 0;
		download_bar.set_message(format!("0/{download_files_total}"));

		while let Some(download_result) = download_futures.join_next().await {
			// The outer Result is task failure (panic/abort), the inner is download failure
			if !matches!(download_result, Ok(Ok(()))) {
				download_bar.finish_and_clear();
				return Err(AlmightyError::Generic("下载一个或多个补丁文件失败！".to_string()));
			}

			download_files_done += 1;
			download_bar.set_message(format!("{download_files_done}/{download_files_total}"));
		}

		download_bar.finish_and_clear();

		// Re-check that GMod wasn't launched during the download phase, since we're about to write to its files
		sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
		if !args.ignore_gmod_running && (sys.processes_by_exact_name("gmod.exe".as_ref()).next().is_some() || sys.processes_by_exact_name("gmod".as_ref()).next().is_some()) {
			return Err(AlmightyError::Generic("😭 Garry's Mod 已在运行，请先关闭它！".to_string()));
		}

		// Patch the files
		terminal_write(writer, format!("\nPatching {pending_files_len} file(s)...").as_str(), true, None);

		let patch_bar = UI.get().unwrap().add(ProgressBar::new(pending_files_len as u64));
		patch_bar.set_style(spinner_style("{spinner} 正在修补 {pos}/{len} 个文件 [{bar:25}] {percent}%"));
		patch_bar.enable_steady_tick(SPINNER_TICK);

		// TODO: Early exit if any patches fail
		let patch_results: Vec<(&String, IntegrityStatus)> = pending_files.par_iter()
		.map(|(filename, integrity_status, hashes)| {
			let new_integrity_status = patch_file(
				writer,
				writer_is_interactive,
				&integrity_status_strings,
				&gmod_path,
				platform_masked,
				&gmod_branch,
				&cache_dir,
				filename,
				integrity_status,
				hashes
			);

			patch_bar.inc(1);

			(*filename, new_integrity_status)
		}).collect();

		patch_bar.finish_and_clear();

		for (_, integrity_status) in patch_results {
			if integrity_status != IntegrityStatus::Fixed {
				return Err(AlmightyError::Generic("修补一个或多个文件失败！".to_string()));
			}
		}

		if args.disable_cache {
			let remove_result = tokio::fs::remove_dir_all(cache_dir).await;

			match remove_result {
				Ok(_) => {
					terminal_write(writer,"\n[disable-cache:Post] 已成功清除 GModPatchTool 缓存目录", true, None);
				},
				Err(error) => {
					terminal_write(writer, format!("\n[disable-cache:Post] 清除 GModPatchTool 缓存目录失败：{error}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
				}
			}
		}
	} else {
		terminal_write(writer, "\n没有文件需要修补，天气真好~", true, Some("green"));
	}

	// Make sure executables are executable on Linux and macOS
	// TODO: Windows support...but at the time of writing it's not well supported in Rust
	// This is done separately because we want it to apply to ALL files regardless of if they needed to be patched
	// https://github.com/solsticegamestudios/GModPatchTool/issues/161
	#[cfg(unix)]
	{
		terminal_write(writer, "\n处理文件权限中...", true, None);

		let mut exec_bit_failed = false;

		for (filename, fileinfo) in platform_branch_files {
			let executable = fileinfo.get("executable");

			if let Some(executable) = executable
				&& executable == "true" {
					let gmod_file_parts: Vec<&str> = filename.split("/").collect();
					let gmod_file_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path.clone(), &gmod_file_parts[..]), true);

					if let Ok(gmod_file_path) = gmod_file_path {
						let metadata = tokio::fs::metadata(&gmod_file_path).await;

						match metadata {
							Ok(metadata) => {
								// Ensure the executable bit is present and apply it to the file
								let mut perms = metadata.permissions();
								let already_executable = perms.mode() & 0o111 != 0;
								perms.set_mode(perms.mode() | 0o111);
								let perms_result: Result<(), io::Error> = tokio::fs::set_permissions(&gmod_file_path, perms).await;

								match perms_result {
									Ok(_) => {
										terminal_write(writer, format!("\t{filename}").as_str(), true, None);
									},
									Err(error) => {
										terminal_write(writer, format!("\t应用权限失败：{filename} | {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });

										// Re-applying perms can fail on already-fine files we don't own; only a missing bit is fatal
										if !already_executable {
											exec_bit_failed = true;
										}
									}
								}
							},
							Err(error) => {
								terminal_write(writer, format!("\t应用权限失败：{filename} | {error}").as_str(), true, if writer_is_interactive { Some("red") } else { None });
							}
						}
					}
				}
		}

		if exec_bit_failed {
			return Err(AlmightyError::Generic("一个或多个文件无法被设置为可执行！".to_string()));
		}
	}

	// Rosetta has to re-translate everything we patch before it can run again, which takes a LONG time for CEF on slower Apple Silicon Macs (and makes the first launch look like it's hanging)
	// Prewarm Rosetta's translation cache now by loading the patched libraries in an x86-64 subprocess, so we can get through it here instead of on first launch
	#[cfg(target_os = "macos")]
	if !args.skip_rosetta_prewarm {
		// Rosetta only exists on Apple Silicon, so this also skips Intel Macs
		if std::path::Path::new("/Library/Apple/usr/share/rosetta/rosetta").is_file() {
			let mut prewarm_paths = Vec::new();
			for (filename, fileinfo) in platform_branch_files {
				let executable = fileinfo.get("executable");

				// Only libraries can be dlopen'd, but the executables are tiny compared to them anyway
				if let Some(executable) = executable
					&& executable == "true"
					&& (filename.ends_with(".dylib") || filename.ends_with("Chromium Embedded Framework")) {
						let gmod_file_parts: Vec<&str> = filename.split("/").collect();
						let gmod_file_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path.clone(), &gmod_file_parts[..]), true);

						if let Ok(gmod_file_path) = gmod_file_path {
							prewarm_paths.push(gmod_file_path);
						}
					}
			}

			// The CEF Framework is by far the biggest, so translate it first in case we hit the timeout
			prewarm_paths.sort_by_key(|prewarm_path| !prewarm_path.ends_with("Chromium Embedded Framework"));

			if !prewarm_paths.is_empty() {
				terminal_write(writer, "\n正在预热 Rosetta 翻译缓存（在较慢的 Mac 上可能需要几分钟）...", true, None);

				match std::env::current_exe() {
					Ok(current_exe) => {
						let mut prewarm_command = tokio::process::Command::new("/usr/bin/arch");
						prewarm_command.arg("-x86_64")
							.arg(current_exe)
							.arg("--rosetta-prewarm")
							.args(&prewarm_paths)
							.kill_on_drop(true);

						let prewarm_result = tokio::time::timeout(time::Duration::from_secs(300), prewarm_command.status()).await;

						match prewarm_result {
							Ok(Ok(prewarm_status)) if prewarm_status.success() => {
								terminal_write(writer, "完成!", true, None);
							},
							Ok(Ok(_)) => {
								terminal_write(writer, "\t部分库预热失败！首次启动时 macOS 翻译新文件可能会变慢。", true, if writer_is_interactive { Some("yellow") } else { None });
							},
							Ok(Err(error)) => {
								terminal_write(writer, format!("\t预热失败: {error}\n\t首次启动时 macOS 翻译新文件可能会变慢。").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
							},
							Err(_) => {
								terminal_write(writer, "\t预热超时！首次启动时 macOS 翻译新文件可能会变慢。", true, if writer_is_interactive { Some("yellow") } else { None });
							}
						}
					},
					Err(error) => {
						terminal_write(writer, format!("\t预热失败: {error}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
					}
				}
			}
		} else if std::env::consts::ARCH == "aarch64" {
			terminal_write(writer, "\n未安装 Rosetta 2！Garry's Mod 在 Apple Silicon 上运行需要它：\n\tsoftwareupdate --install-rosetta --agree-to-license", true, if writer_is_interactive { Some("yellow") } else { None });
		}
	}

	// Delete ChromiumCache/ChromiumCacheMultirun/chromium.log
	// Solves issues with being corrupt/stuck lockfiles, and GMod MUST NOT be running for this tool to run, so it probably solves more issues than it could create
	if !args.skip_clear_chromiumcache {
		let gmod_chromiumcache_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path.clone(), &["ChromiumCache"]), false);
		if let Ok(gmod_chromiumcache_path) = gmod_chromiumcache_path {
			terminal_write(writer, "\n正在清理 ChromiumCache...", true, None);
			if let Err(error) = tokio::fs::remove_dir_all(gmod_chromiumcache_path).await {
				terminal_write(writer, format!("\t错误: {error}\n\t你可能需要手动从 GarrysMod 目录中删除 ChromiumCache").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
			} else {
				terminal_write(writer, "完成!", true, None);
			}
		}

		let gmod_chromiumcachemultirun_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path.clone(), &["ChromiumCacheMultirun"]), false);
		if let Ok(gmod_chromiumcachemultirun_path) = gmod_chromiumcachemultirun_path {
			terminal_write(writer, "\n正在清理 ChromiumCacheMultirun...", true, None);
			if let Err(error) = tokio::fs::remove_dir_all(gmod_chromiumcachemultirun_path).await {
				terminal_write(writer, format!("\t错误: {error}\n\t你可能需要手动从 GarrysMod 目录中删除 ChromiumCacheMultirun").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
			} else {
				terminal_write(writer, "完成!", true, None);
			}
		}

		let gmod_chromiumlog_path = path_to_canonical_pathbuf(extend_pathbuf_and_return(gmod_path.clone(), &["chromium.log"]), false);
		if let Ok(gmod_chromiumlog_path) = gmod_chromiumlog_path {
			terminal_write(writer, "\n正在删除 chromium.log...", true, None);
			if let Err(error) = tokio::fs::remove_file(gmod_chromiumlog_path).await {
				terminal_write(writer, format!("\t错误: {error}\n\t你可能需要手动从 GarrysMod 目录中删除 chromium.log").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
			} else {
				terminal_write(writer, "完成!", true, None);
			}
		}
	}

	// TODO: Update BASS? https://github.com/Facepunch/garrysmod-requests/issues/1885
	// TODO: Check dxlevel/d3d9ex support in Proton, and if there's anything we can do about it

	let now = now.elapsed().as_secs_f64();
	terminal_write(writer, format!("\nGModPatchTool 已成功运行！花费时间：{now} 秒！").as_str(), true, if writer_is_interactive { Some("green") } else { None });

	if args.launch_gmod {
		terminal_write(writer, "正在启动 Garry's Mod...", true, if writer_is_interactive { Some("green") } else { None });

		let open_result = open::that_detached("steam://rungameid/4000");
		if let Err(error) = open_result {
			terminal_write(writer, format!("\t失败: {error}").as_str(), true, if writer_is_interactive { Some("yellow") } else { None });
		}
	} else {
		terminal_write(writer, "你现在可以通过 Steam 启动 GMod 了", true, if writer_is_interactive { Some("green") } else { None });
	}

	// Keep the emoji outside the colored span: bold color codes break color emoji in the GUI terminal
	terminal_write(writer, "\n💖 ", false, None);
	terminal_write(writer, "觉得这个工具好用的话，请通过下面的链接支持我们！", true, if writer_is_interactive { Some("magenta") } else { None });
	terminal_write(writer, "\t软件原作者 > https://solsticegamestudios.com/donate/", true, None);
	terminal_write(writer, "\n\n软件的运营需要成本，如果你有能力，请帮帮我们！", true, if writer_is_interactive { Some("magenta") } else { None });
	terminal_write(writer, "\t赞助链接 > https://afdian.com/a/eh5here", true, None);
	terminal_write(writer, "\n求求啦，谢谢啦 🙏🙏🙏", true, None);

	Ok(())
}

// Prewarm child mode: dlopen each library given on the command line, which makes Rosetta translate it into its cache before returning
// dlopen can only load x86-64 libraries from an x86-64 process, so the prewarm step above runs our own x86-64 slice via /usr/bin/arch
// This MUST run before everything else in main() so the child doesn't continue into the GUI/lockfile/arg stuff
#[cfg(target_os = "macos")]
fn rosetta_prewarm_child() {
	let mut args = std::env::args_os();

	if args.nth(1).as_deref() != Some(std::ffi::OsStr::new("--rosetta-prewarm")) {
		return;
	}

	unsafe extern "C" {
		fn dlopen(filename: *const std::ffi::c_char, flags: std::ffi::c_int) -> *mut std::ffi::c_void;
		fn dlerror() -> *const std::ffi::c_char;
	}
	const RTLD_NOW: std::ffi::c_int = 0x2;

	let mut prewarm_failed = false;

	for path in args {
		let path_cstring = std::ffi::CString::new(path.as_encoded_bytes());

		match path_cstring {
			Ok(path_cstring) => {
				// Loading the library is all it takes; we don't need to do anything with it
				let dlopen_result = unsafe { dlopen(path_cstring.as_ptr(), RTLD_NOW) };

				if dlopen_result.is_null() {
					let dlopen_error = unsafe { dlerror() };
					let dlopen_error = if dlopen_error.is_null() { std::borrow::Cow::from("未知错误") } else { unsafe { std::ffi::CStr::from_ptr(dlopen_error) }.to_string_lossy() };
					println!("\t转译失败: {} | {dlopen_error}", path.to_string_lossy());
					prewarm_failed = true;
				} else {
					println!("\t已转译: {}", path.to_string_lossy());
				}
			},
			Err(error) => {
				println!("\t转译失败: {} | {error}", path.to_string_lossy());
				prewarm_failed = true;
			}
		}
	}

	std::process::exit(if prewarm_failed { 1 } else { 0 });
}

fn delete_pid_lockfile() {
	let os_cache_dir = if let Some(dirs_cache_dir) = dirs::cache_dir() { dirs_cache_dir } else { std::env::temp_dir() };
	let pid_path = extend_pathbuf_and_return(os_cache_dir.clone(), &["gmodpatchtool.pid"]);
	let running_instance_pid = std::fs::read_to_string(&pid_path);

	if let Ok(pid) = running_instance_pid
		&& let Ok(pid) = pid.parse::<u32>()
			&& pid == std::process::id() {
				let pid_remove_result = std::fs::remove_file(&pid_path);
				if let Err(error) = pid_remove_result {
					println!("移除 gmodpatchtool.pid 时失败: {error}");
				}
			}
}

fn terminal_exit_handler() {
	println!("\n按回车键退出程序...");
	std::io::stdin().read_line(&mut String::new()).unwrap();
	delete_pid_lockfile();
}

fn main_script<W>(writer: fn() -> W, writer_is_interactive: bool, args: Args) -> Result<(), AlmightyError>
where
	W: std::io::Write + 'static
{
	// Run the script on a thread with a bigger stack, since VDF parsing can overflow the default (only 1 MiB on Windows)
	// block_on polls the future on the calling thread, so the runtime's thread_stack_size only covers its worker threads
	// TODO: Report localconfig.vdf/config.vdf overflow: https://github.com/CosmicHorrorDev/vdf-rs/issues
	std::thread::Builder::new()
		.stack_size(0x800000) // 8 MiB
		.spawn(move || {
			tokio::runtime::Builder::new_multi_thread()
				.enable_all()
				.thread_stack_size(0x800000) // 8 MiB
				.build()
				.map_err(|error| AlmightyError::Generic(format!("创建 Tokio 运行时失败: {error}")))?
				.block_on(
					main_script_internal(writer, writer_is_interactive, args)
				)
		})
		.map_err(|error| AlmightyError::Generic(format!("创建修补线程失败: {error}")))?
		.join()
		.map_err(|_| AlmightyError::Generic("修补线程发生崩溃".to_string()))?
}

fn init_logger<W>(ansi: bool, writer: fn() -> W)
where
	W: std::io::Write + 'static
{
	tracing_subscriber::fmt()
		.with_env_filter(EnvFilter::from_default_env())
		.with_ansi(ansi)
		.without_time()
		.with_target(false)
		.with_writer(writer)
		.init();
}

pub fn main() {
	// Exits the process if this is a prewarm child
	#[cfg(target_os = "macos")]
	rosetta_prewarm_child();

	#[cfg(target_os = "windows")]
	use crossterm::ansi_support::supports_ansi;
	#[cfg(not(target_os = "windows"))]
	fn supports_ansi() -> bool { true }

	let is_terminal = io::stdout().is_terminal();
	let is_ansi = is_terminal && supports_ansi();

	init_logger(is_ansi, std::io::stdout);

	{
		use std::{env, process};

		#[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
		let mut force_gui = match env::var("FORCE_GUI") {
			Ok(value) => Some(value.trim() == "1"),
			Err(env::VarError::NotPresent) => None,
			Err(error) => {
				error!("FORCE_GUI is invalid: {error}");
				process::exit(1);
			},
		};

		#[cfg(target_os = "windows")]
		{
			use win32console::console::WinConsole;
			match WinConsole::get_process_list() {
				Ok(list) if list.len() == 1 => {
					force_gui = Some(true);
					if let Err(error) = WinConsole::free_console() {
						tracing::warn!("GUI | {error}");
					}
				},
				Ok(_) => {},
				Err(error) => {
					tracing::warn!("GUI | {error}");
				}
			}
		}

		if force_gui.unwrap_or(!is_terminal || !is_ansi) {
			// TODO: Make this safe if possible
			// https://doc.rust-lang.org/std/env/fn.set_var.html
			unsafe {
				env::set_var("FORCE_GUI", "0");
			}

			if let Err(error) = gui::main() {
				error!("GUI | {error}");
				process::exit(1);
			}

			process::exit(0);
		}
	}

	if is_ansi {
		print!("\x1B]0;GModPatchTool\x07");
	}

	let writer = std::io::stdout;
	let writer_is_interactive = is_terminal;

	// Write about
	terminal_write(writer, ABOUT, true, if writer_is_interactive { Some("cyan") } else { None });

	// Parse the args
	let args = match Args::try_parse() {
		Ok(args) => args,
		Err(error) => {
			let _ = error.print();
			terminal_exit_handler();
			std::process::exit(error.exit_code());
		},
	};

	let skip_exit_prompt = args.skip_exit_prompt;

	let script_result = main_script(writer, writer_is_interactive, args);
	if let Err(error) = &script_result {
		error!("{error}");
	}

	if is_terminal && !skip_exit_prompt {
		terminal_exit_handler();
	} else {
		delete_pid_lockfile();
	}

	// Exit non-zero on failure
	if script_result.is_err() {
		std::process::exit(1);
	}
}

#[cfg(test)]
mod tests;

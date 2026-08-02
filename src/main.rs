#![cfg_attr(target_os = "windows", windows_subsystem = "console")]

// The features are mutually exclusive; generate must be built with --no-default-features
#[cfg(all(feature = "generate", feature = "patch"))]
compile_error!("The generate feature requires --no-default-features");

fn main() {
	// Print a backtrace on any panic so crash reports from the wild are debuggable without RUST_BACKTRACE
	std::panic::set_hook(Box::new(|panic_info| {
		eprintln!("\n{panic_info}");
		eprintln!("\n回溯:\n{}", std::backtrace::Backtrace::force_capture());
		eprintln!("GModPatchTool {} ({} {}) 崩溃了！请将上方完整内容反馈给我们：", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
		eprintln!("\tQQ群: 105969906");
		eprintln!("\t邮箱: admin@nekogan.com");
	}));

	#[cfg(feature = "generate")]
	gmodpatchtool::generate::main();

	#[cfg(feature = "patch")]
	gmodpatchtool::patch::main();
}

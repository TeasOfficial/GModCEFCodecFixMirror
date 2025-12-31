#[cfg(feature = "generate")]
pub mod generate;

#[cfg(feature = "patch")]
pub mod patch;

#[cfg(feature = "patch")]
mod gui;

#[cfg(feature = "patch")]
mod vdf;

const ABOUT: &str = r#"   ________  ___          ______        __       __  ______            __
  / ____/  |/  /___  ____/ / __ \____ _/ /______/ /_/_  __/___  ____  / /
 / / __/ /|_/ / __ \/ __  / /_/ / __ `/ __/ ___/ __ \/ / / __ \/ __ \/ /
/ /_/ / /  / / /_/ / /_/ / ____/ /_/ / /_/ /__/ / / / / / /_/ / /_/ / /
\____/_/  /_/\____/\__,_/_/    \__,_/\__/\___/_/ /_/_/  \____/\____/_/

版权所有 2020-2025, Solstice Game Studios (solsticegamestudios.com)
开源协议 GNU General Public License v3.0

修复：初雪 OriginalSnow


国内发行版联系方式：
	> 交流群：105969906
	> 邮箱：admin@nekogan.com
论坛：https://bbs.ipairsdo.xin

友情链接：
	> 申必の果茶店（QQ群）：920655299
"#;

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use indexmap::IndexMap;
use rayon::prelude::*;

type Manifest = IndexMap<String, IndexMap<String, IndexMap<String, IndexMap<String, String>>>>;

fn pathbuf_dir_not_empty(pathbuf: &Path) -> bool {
	// If this is a valid file in the directory, the directory isn't empty
	if pathbuf.is_file() {
		return true;
	}

	let pathbuf_dir = pathbuf.read_dir();

	pathbuf_dir.is_ok() && pathbuf_dir.unwrap().next().is_some()
}

fn pathbuf_to_canonical_pathbuf(pathbuf: PathBuf, checkdirempty: bool) -> Result<PathBuf, String> {
	#[cfg(windows)]
	use dunce::canonicalize;
	#[cfg(not(windows))]
	let canonicalize = Path::canonicalize;

	let pathbuf_result = canonicalize(pathbuf.as_path());

	match pathbuf_result {
		Ok(pathbuf) => {
			if !checkdirempty || pathbuf_dir_not_empty(&pathbuf) {
				Ok(pathbuf)
			} else {
				Err("Directory is empty".to_string())
			}
		},
		Err(error) => {
			Err(error.to_string())
		}
	}
}

fn string_to_canonical_pathbuf(path_str: String) -> Option<PathBuf> {
	#[cfg(windows)]
	use dunce::canonicalize;
	#[cfg(not(windows))]
	let canonicalize = Path::canonicalize;

	let pathbuf_result = canonicalize(Path::new(&path_str));

	if let Ok(pathbuf) = pathbuf_result {
		if pathbuf_dir_not_empty(&pathbuf) {
			return Some(pathbuf);
		}
	}

	None
}

fn extend_pathbuf_and_return(mut pathbuf: PathBuf, segments: &[&str]) -> PathBuf {
	pathbuf.extend(segments);

	pathbuf
}

fn get_file_hash(file_path: &PathBuf) -> Result<String, String> {
	let mut hasher = blake3::Hasher::new();
	let hash_result = hasher.update_mmap_rayon(file_path);

	match hash_result {
		Ok(_) => {
			Ok(format!("{}", hasher.finalize()))
		},
		Err(error) => {
			Err(error.to_string())
		}
	}
}

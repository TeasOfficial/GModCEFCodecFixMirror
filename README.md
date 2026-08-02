# GModPatchTool 镜像版 <sub>_曾是 GModCEFCodecFix_</sub>

最新版下载：https://nekogan.com/gccf

![GModPatchTool](GModPatchToolLogo.png)

**由 Solstice Game Studios 制作 (solsticegamestudios.com)**

# 🛠️ 补丁内容
### 1. 对于所有平台
- 修复了 Linux/macOS 上潜在的启动问题，如：主菜单丢失、启动失败等
- 新增参数 `-chromium_fps_max`
  - 为所有的 CEF 页面设置最大帧率上限！
  - 通过降低页面帧率，可能会提高游戏帧率
  - 默认为 60
- 通过我们全新的 SourceScheme.res，重置了传统 VGUI 组件！
- 将开发者控制台的字体修改为 [PT Mono](https://fonts.google.com/specimen/PT+Mono)，以提高所有平台的一致性/可读性（镜像版日后可能会支持自定义字体）
  - 这对于 Proton 特别重要，因为使用这些字体的文本在默认情况下会显示不正常或非常小（没有 Lucida Console）
  - 如果你不喜欢字体替换或 VGUI 重置，可以在修补器中添加启动参数 `--no-sourcescheme` 来跳过这一项，我们也会在安装过程中询问你（镜像版独占）

### 2. 游戏内浏览器 ([Chromium 嵌入式框架, 也叫 CEF](https://en.wikipedia.org/wiki/Chromium_Embedded_Framework))
- 将 CEF 框架内核更新为 137.0.10 版本（Chromium 137.0.7151.69）
- 启用专有音视频编解码器（Proprietary Video/Audio codec），为 H.264 (MP4) 与 AAC 等格式添加支持！
- 启用了 [Widevine](https://www.widevine.com) 支持（但没有 [VMP](https://github.com/solsticegamestudios/GModPatchTool/issues/100)，Netflix 等服务目前仍无法使用）
- 启用软件 WebGL
- 支持部分 GPU 加速
- 提高纹理更新性能
- 禁用硬件多媒体键对媒体的控制
- 重新启用站点隔离（安全功能；某些网站需要它才能正常运行）

### 3. Linux

<sub>_此处内容的技术含量较高，通常不推荐阅读_
- 可以修复 Steam Overlay/MangoHud 等不工作的问题
- 在 GMod 的启动选项中输入 `GMOD_ENABLE_LD_PRELOAD=1 %command%` 尝试解决！
  - 默认情况下此功能是禁用的，因为它可能会直接导致 GMod 崩溃
- 设置 `mesa_glthread=true`，使用 Mesa 驱动获得更好的 OpenGL 性能
- 设置 `ulimit -n $(ulimit -Hn)` 以解决打开/挂载大量文件（许多插件、Lua 自动刷新等）的问题
- 在 `hl2.sh` 中添加了带注释的环境变量导出，帮助多 GPU 用户（通常是笔记本用户）让 GMod 使用正确的 GPU
  - 请参阅 [#188](https://github.com/solsticegamestudios/GModPatchTool/issues/188) 了解我们为什么默认不启用这些功能

# ❓ 如何使用
推荐：下载 **[最新构建](https://github.com/solsticegamestudios/GModPatchTool/releases)** 并运行应用程序

需要更多技术支持？请访问 https://solsticegamestudios.com/fixmedia/
或加入国内 QQ 群：105969906

# 👩‍💻 以下内容仅对开发者有效
### 如何使用？
阅读前请注意：**该补丁仅在客户端生效！！！**

**如何检测是否已安装 CEF：** 来看我们的 Lua 示例！[是的，我就是示例，点我！](examples/detection_example.lua)

> [!警告]
> 我们的 CEF 构建启用了站点隔离，这意味着**你必须注意调用与 JavaScript 相关的 DHTML 函数的位置！**
>
> 如果你需要使用 [DHTML.AddFunction](https://wiki.facepunch.com/gmod/DHTML:AddFunction)、[DHTML.QueueJavascript](https://wiki.facepunch.com/gmod/DHTML:QueueJavascript) 或 [DHTML.RunJavascript](https://wiki.facepunch.com/gmod/Panel:RunJavascript) 函数，请确保它们在页面加载完成后再调用，否则函数不会生效！可以通过检查它们是否在 [HTML.OnBeginLoadingDocument](https://wiki.facepunch.com/gmod/HTML:OnBeginLoadingDocument) 之后调用来确认。
>
> 站点隔离在导航时会销毁 JavaScript 状态，就像真实的网页浏览器一样。
>
> 该工具包含一个针对 mainmenu.lua 的补丁，用于解决 GMod 自身调用方法不正确的问题，但**对于未正确处理 JS HTML 面板状态的插件来说，这是一个重大更改**。

**想获取更多信息：** 请查看我们在 GitHub 上 fork 的仓库 [gmod-html](https://github.com/solsticegamestudios/gmod-html) 与 [CEF 构建脚本](cef_build)

# 📢 出现错误? / 联系我们
* (官方)常见问题与解决方案: https://solsticegamestudios.com/fixmedia/faq/
* (官方)Discord: https://solsticegamestudios.com/discord/
* (官方)Email: contact@solsticegamestudios.com
* (国内)QQ群：105969906
* (国内)Email: admin@nekogan.com

# 💖 结语
本程序完全开源且免费。如果你通过其他渠道购买到了本软件，请尽快退款并举报！我们对倒卖行为绝对零容忍！

<!-- **If you like what we're doing here, consider [throwing a few dollars our way](https://solsticegamestudios.com/donate/)!** Our work is 100% funded by users of the tool! -->
# 💰 支持者列表
* Solstice Game Studios（原作者，感谢制作了该软件）

# GModPatchTool 镜像版 <sub>_曾是 GModCEFCodecFix_</sub>

![GModPatchTool](GModPatchToolLogo.png)

**由 Solstice Game Studios 制作 (solsticegamestudios.com)**

# 🛠️ 补丁内容
### 1. 对于所有平台
- 修复了 Linux/macOS 上潜在的启动问题，如：主菜单丢失、启动失败等
- 新增参数 `-chromium_fps_max`
  - 为所有的 CEF 页面设置最大帧率上限！
  - 通过降低页面帧率，这将(可能)会提高游戏帧率
  - 默认为 60
- 通过我们船新版本的 SourceScheme.res，重置了传统的 VGUI 组件！
- 将 开发者控制台 的字体修改为 [PT Mono](https://fonts.google.com/specimen/PT+Mono) 来提高所有平台的一致性/可读性（镜像版日后可能会支持自定义字体）
  - 这对于 Proton 特别重要，因为使用这些字体的文本在默认情况下会显示不正常或非常小（没有 Lucida Console）
  - 如果你不喜欢字体替换或VGUI重置，可以通过在修补器中添加启动项 `--no-sourcescheme` 来规避这一项安装，我们也会在安装过程中询问你（镜像版独占）

### 2. 游戏内浏览器 ([Chromium 嵌入式框架, 也叫 CEF](https://en.wikipedia.org/wiki/Chromium_Embedded_Framework))
- 更新框架内核 CEF 为 137.0.10 版本 (Chromium 137.0.7151.69)
- 启用进阶编解码器 [Proprietary Video/Audio codec](https://www.chromium.org/audio-video), 为诸如 H.264 (MP4) 与 AAC 添加了支持！
- 启用 [Widevine](https://www.widevine.com) 支持 (但是没有 [VMP](https://github.com/solsticegamestudios/GModPatchTool/issues/100)，所以 Netflix 等服务目前无法使用……)
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
- 将 `mesa_glthread=true` 设置为使用 Mesa 驱动获得更好的 OpenGL 性能
- 设置 `ulimit -n $(ulimit -Hn)` 以解决打开/挂载大量文件（许多插件、Lua 自动刷新等）的问题
- 在 `hl2.sh` 中添加了各种带注释的导出，以帮助多 GPU 用户快速指向 GMod 使用正确的 GPU（通常是笔记本电脑）
  - 请参阅 [#188](https://github.com/solsticegamestudios/GModPatchTool/issues/188) 了解我们为什么默认不启用这些功能

# ❓ 如何使用
推荐：下载 **[最新构建](https://github.com/solsticegamestudios/GModPatchTool/releases)** 并运行应用程序

需要更多的技术支持？请访问 https://solsticegamestudios.com/fixmedia/
或添加国内QQ群聊获取更多支持 105969906

# 👩‍💻 以下内容仅对开发者有效<br>如何使用？
阅读前，你有义务让玩家遵循玩家使用手册，**该补丁仅在 客户端 有效！！！**

**如何检测是否已安装 CEF:** 来看我们的Lua示例！ [是的，我是示例，点我！](examples/detection_example.lua)

> [!警告]
> 我们的 CEF 构建启用了站点隔离，这意味着**你必须注意调用与 JavaScript 相关的 DHTML 函数的位置！**
>
> 如果你需要使用 [DHTML.AddFunction](https://wiki.facepunch.com/gmod/DHTML:AddFunction), [DHTML.QueueJavascript](https://wiki.facepunch.com/gmod/DHTML:QueueJavascript) 或 [DHTML.RunJavascript](https://wiki.facepunch.com/gmod/Panel:RunJavascript) 函数的话，请确保他们在页面加载完成后再调用，否则函数将不会生效！你可以通过检查它们是否在 [HTML.OnBeginLoadingDocument](https://wiki.facepunch.com/gmod/HTML:OnBeginLoadingDocument) 之后调用来确保这一点。
>
> 站点隔离在导航时会销毁 JavaScript 状态，就像真实的网页浏览器一样。
>
> 该工具包含了一个针对 mainmenu.lua 的补丁，用于解决 GMod 自身未使用正确方法的问题，但**对于未正确处理 JS HTML 面板状态的任何插件来说，这是一个重大更改**。

**如果你想获取到更多信息:** 请查看我们在GitHub上fork的仓库 [gmod-html](https://github.com/solsticegamestudios/gmod-html) 与 [CEF构建脚本](cef_build)

# 📢 出现错误? / 联系我们
* (官方)常见问题与解决方案: https://solsticegamestudios.com/fixmedia/faq/
* (官方)Discord: https://solsticegamestudios.com/discord/
* (官方)Email: contact@solsticegamestudios.com
* (国内)QQ群：105969906
* (国内)Email: admin@nekogan.com

# 💖 结语
该程序完全开源且免费，如果你通过其他任何渠道购买到此软件，请尽快退款并举报！我们对倒卖行为绝对零容忍！

<!-- **If you like what we're doing here, consider [throwing a few dollars our way](https://solsticegamestudios.com/donate/)!** Our work is 100% funded by users of the tool! -->
# 💰 支持者列表
* Solstice Game Studios (原作者，感谢制作了该软件)

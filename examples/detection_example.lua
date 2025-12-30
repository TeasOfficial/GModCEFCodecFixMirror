--[[
	GModPatchTool (曾是 GModCEFCodecFix) 检测代码示例

	版权所有 2024-2025, Solstice Game Studios (solsticegamestudios.com)
	开源协议 GNU General Public License v3.0

	目的：检测 GModPatchTool 的 CEF 补丁是否已成功应用于 GMod 客户端

	联系我们:
		仓库: https://github.com/TeasOfficial/GModPatchToolMirror/
		Discord: https://solsticegamestudios.com/discord/
		邮箱: contact@solsticegamestudios.com
]]

-- CEF 仅在客户端上运行
if not CLIENT then return end

-- 在你的 Lua 代码的其他地方使用这些全局变量进行检测
CEFAvailable = BRANCH == "x86-64" or system.IsWindows()
CEFCodecFixChecked = false
CEFCodecFixAvailable = false

-- 我们通过 hook PreRender 来检测可用性
hook.Add("PreRender", "CEFCodecFixCheck", function()
	hook.Remove("PreRender", "CEFCodecFixCheck")

	print("Querying CEF Codec Support...")

	-- 如果客户端没有使用包含 CEF 的测试版，他们就不可能拥有 CEFCodecFix
	if not CEFAvailable then
		CEFCodecFixAvailable = false
		CEFCodecFixChecked = true
		print("CEF does not have CEFCodecFix")
		hook.Run("CEFCodecFixStatus", CEFAvailable, CEFCodecFixAvailable)
		return
	end

	local cefTestPanel = vgui.Create("DHTML", nil, "CEFCodecFixCheck")
	cefTestPanel:SetSize(32, 32)
	cefTestPanel:SetKeyboardInputEnabled(false)
	cefTestPanel:SetMouseInputEnabled(false)
	function cefTestPanel:Paint()
		return true -- 不用正常绘制这个面板
	end
	function cefTestPanel:RemoveWhileHidden()
		-- 面板显然在调用 Remove() 后会绘制一帧，因此我们提前禁用可见性
		-- 注意：不要使用 SetVisible(false) 来替代 Paint 覆盖！没有面板“可见性”，Panel Think/JavaScript 将无法运行
		self:SetVisible(false)
		self:Remove()
	end

	cefTestPanel:SetHTML("")

	function cefTestPanel:OnDocumentReady()
		if not CEFCodecFixChecked then
			self:AddFunction("gmod", "getCodecStatus", function(codecStatus)
				CEFCodecFixAvailable = codecStatus
				CEFCodecFixChecked = true

				if CEFCodecFixAvailable then
					print("CEF 已通过 CEFCodecFix 修补")
				else
					print("CEF 未通过 CEFCodecFix 修补")
				end

				hook.Run("CEFCodecFixStatus", CEFAvailable, CEFCodecFixAvailable)
				self:RemoveWhileHidden()
			end)

			---- 这实际上是通过查看网页框架是否能够播放 H.264（一种专有视频编解码器）来进行检测的
			self:QueueJavascript([[gmod.getCodecStatus(document.createElement("video").canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"') == "probably")]])
		elseif IsValid(self) then
			self:RemoveWhileHidden()
		end
	end
end)

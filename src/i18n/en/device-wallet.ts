export const deviceWallet: Record<string, string> = {
  "设备钱包": "Device Wallet",
  "免注册 · 随设备使用": "No signup · works with this device",
  "Key 就是钱包凭证：充值、备份、换电脑和多个便携副本共用，都围绕这一把 Key。":
    "The key is the wallet credential. Top-ups, backups, moving computers, and sharing portable copies all use this one key.",
  "一键充值": "Top up",
  "设备钱包 Key 已复制，请把它保存在安全的地方": "Device Wallet key copied. Store it somewhere safe.",
  "复制失败，请手动选中 Key 复制": "Copy failed. Select and copy the key manually.",
  "填入你已有的设备钱包 Key。换电脑或多个便携副本共用余额时，填同一把 Key 即可。":
    "Enter an existing Device Wallet key. Use the same key to keep your balance when moving computers or using multiple portable copies.",
  "请先填入已有设备钱包 Key": "Enter an existing Device Wallet key first.",
  "无法读取剪贴板，请在输入框中手动粘贴": "Could not read the clipboard. Paste into the field manually.",
  "使用这把 Key 替换本机当前设备钱包？请先确认当前 Key 已备份。":
    "Replace this machine's current Device Wallet with this key? Make sure the current key is backed up first.",
  "已启用已有设备钱包": "Existing Device Wallet enabled",
  "填入已有 Key 失败：": "Could not use the existing key: ",
  "更换设备钱包 Key？余额会保留，旧 Key 会失效；其它电脑或脚本需要改填新 Key。":
    "Rotate the Device Wallet key? The balance is preserved, the old key is revoked, and other computers or scripts must be updated.",
  "设备钱包 Key 已更新": "Device Wallet key updated",
  "更新设备钱包 Key 失败：": "Could not update the Device Wallet key: ",
  "当前 Key 已自动复制，请先保存好再确认删除": "The current key was copied automatically. Save it before confirming removal.",
  "无法自动复制当前 Key，已取消删除；请先手动复制": "The current key could not be copied, so removal was cancelled. Copy it manually first.",
  "只从这台机器移除设备钱包？服务端钱包、旧 Key 和余额不会删除。下次联网会生成一个新的零余额钱包；你仍可用刚复制的 Key 找回原钱包。":
    "Remove the Device Wallet only from this machine? The server wallet, old key, and balance are not deleted. The next online run creates a new zero-balance wallet; use the copied key to restore the original wallet.",
  "已从本机移除设备钱包": "Device Wallet removed from this machine",
  "移除本机钱包失败：": "Could not remove the local wallet: ",
  "检测到旧钱包本机记录曾丢失。原余额无法自动找回，请凭充值订单号联系客服迁移。":
    "The old wallet's local record was lost. Its balance cannot be restored automatically; contact support with the top-up order number.",
  "当前余额": "Current balance",
  "待充值": "Not topped up",
  "余额偏低，长对话可能不够一次预扣": "Low balance; a long conversation may exceed one pre-authorization",
  "余额永久有效，不用不扣": "Balance never expires and is charged only when used",
  "充值到账后即可调用 AI": "Use AI after the top-up arrives",
  "当前设备钱包 Key": "Current Device Wallet key",
  "正在联网生成…": "Creating online…",
  "钱包编号": "Wallet ID",
  "保存在本机，不上传到聊天记录": "Stored locally and never added to chat history",
  "复制中…": "Copying…",
  "粘贴中…": "Pasting…",
  "粘贴": "Paste",
  "填入已有设备钱包 Key": "Enter an existing Device Wallet key",
  "启用已有 Key": "Use existing key",
  "更换 Key": "Rotate key",
  "移除中…": "Removing…",
  "移除本机钱包": "Remove local wallet",
  "移除本机钱包不会删除服务端钱包或余额。请先保存 Key；以后填回同一把 Key 即可恢复使用。":
    "Removing the local wallet does not delete the server wallet or balance. Save the key first; enter the same key later to restore access.",
  // 2026-08-22：钱包收敛到 WalletCard 时，「移除本机钱包」那条安全带（先复制、复制不成功
  // 就不许继续）跟着搬过来，下面四条是它的文案。
  "当前 Key 已自动复制，请先保存好再确认移除":
    "Your current key has been copied — save it somewhere safe before confirming removal",
  "无法自动复制当前 Key，已取消移除；请先手动复制":
    "Could not copy the current key, so removal was cancelled — please copy it manually first",
  "只从这台机器移除设备钱包？服务端钱包、旧 Key 和余额不会删除。\n下次联网会生成一把新的零余额 Key；你仍可用刚复制的这把找回原钱包。":
    "Remove the Device Wallet from this computer only? The server wallet, the old key and the balance are not deleted.\nA new key with zero balance will be created next time you go online; the key you just copied still restores the original wallet.",
  "服务端钱包和余额不会删除；移除前会先把当前 Key 复制给你":
    "The server wallet and balance are kept; your current key is copied for you before removal",
};

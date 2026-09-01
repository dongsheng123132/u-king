r"""
生成「干净机器上验任务看板」的 PowerShell 脚本（配合 aliyun-clean-windows-test skill）。

为什么要绕这一道：第一版直接在 .ps1 里手写 JSON 字符串，反斜杠在
bash heredoc → aliyun CLI → PowerShell 这条链上被吃成单个 `\`，
`"cwd":"D:\work\alpha"` 成了非法 JSON —— 五家里三家的首行当场解析失败。
（那一跑倒是白捡一个结论：喂烂数据不崩、优雅退化。但它验不了解析器。）

所以改成：**本地按正确 JSON 造好 → base64 → 到机器上再解码落盘**，
中间任何一层都碰不到转义。

用法：
    python scripts/gen-aitasks-cleanroom.py > /tmp/test-aitasks.ps1
    bash ~/.claude/skills/aliyun-clean-windows-test/scripts/clean-win.sh run /tmp/test-aitasks.ps1
"""

import base64
import json

SANDBOX = "C:\\uking-sandbox"
EXE = "C:\\uk\\U-King.exe"


def j(o):
    return json.dumps(o, ensure_ascii=False)


FILES = {
    # —— Claude Code：人当场敲的那条带 origin.kind=human ——
    ".claude\\projects\\proj-a\\s1.jsonl":
        j({"type": "user", "sessionId": "c1", "cwd": "D:\\work\\alpha",
           "timestamp": "2026-08-01T00:00:00.000Z", "origin": {"kind": "human"},
           "message": {"role": "user", "content": "把合同里的甲方改一下"}}) + "\n" +
        j({"type": "assistant", "message": {"model": "claude-opus-5"}}) + "\n",
    # 定时 / 无头跑起来的会话：isMeta:true、没有 origin —— 标题必须仍然取得到
    ".claude\\projects\\proj-a\\s2.jsonl":
        j({"type": "user", "sessionId": "c2", "cwd": "D:\\work\\beta", "isMeta": True,
           "message": {"role": "user", "content": "每日巡视"}}) + "\n",
    # 子代理记录：带**父会话的 id**，必须被深度上限挡在外面（否则 id 撞车）
    ".claude\\projects\\proj-a\\c1\\subagents\\agent-x.jsonl":
        j({"type": "user", "sessionId": "c1", "cwd": "D:\\work\\alpha", "origin": {"kind": "human"},
           "message": {"role": "user", "content": "子代理内部"}}) + "\n",

    # —— Codex：标题只能来自 event_msg/user_message ——
    ".codex\\sessions\\2026\\08\\01\\rollout-x.jsonl":
        j({"type": "session_meta", "payload": {"type": "session_meta", "session_id": "x1",
           "cwd": "D:\\work\\gamma", "timestamp": "2026-08-01T00:00:00.000Z",
           "model": "gpt-5.4-codex"}}) + "\n" +
        j({"type": "response_item", "payload": {"type": "message", "role": "user",
           "content": [{"type": "input_text",
                        "text": "<recommended_plugins>别拿我当标题</recommended_plugins>"}]}}) + "\n" +
        j({"type": "event_msg", "payload": {"type": "user_message",
           "message": "<codex_delegation><input>跑一遍服务器巡检</input></codex_delegation>"}}) + "\n",

    # —— Hermes：真会话 vs 出错转储（转储绝不能变成一张卡）——
    ".hermes\\sessions\\session_20260801_000000_000000.json":
        j({"session_id": "h1", "model": "deepseek-v4-flash",
           "session_start": "2026-08-01T00:00:00.000Z",
           "messages": [{"role": "user", "content": "写个周报"}]}),
    ".hermes\\sessions\\request_dump_20260801_000000_000000.json":
        j({"session_id": "hd", "reason": "error", "request": {}, "error": "boom"}),

    # —— OpenClaw / ClawX：两个 home 都要认，轨迹文件不能重复计数 ——
    ".openclaw\\agents\\main\\sessions\\o1.jsonl":
        j({"type": "session", "id": "o1", "cwd": "D:\\work\\delta",
           "timestamp": "2026-08-01T00:00:00.000Z"}) + "\n" +
        j({"type": "model_change", "modelId": "deepseek-v4-pro"}) + "\n" +
        j({"type": "message", "message": {"role": "user", "content": "做一张宣传海报"}}) + "\n",
    ".openclaw\\agents\\main\\sessions\\o1.trajectory.jsonl":
        j({"type": "session", "id": "o1"}) + "\n",
    # U-King 便携 home（内置终端用的那个）——只扫默认 home 的话这条会消失
    ".uking\\openclaw\\agents\\main\\sessions\\o2.jsonl":
        j({"type": "session", "id": "o2", "cwd": "D:\\work\\eps",
           "timestamp": "2026-08-01T00:00:00.000Z"}) + "\n" +
        j({"type": "message", "message": {"role": "user",
           "content": [{"type": "text", "text": "剪个短视频"}]}}) + "\n",

    # —— AI 自己写的看板：状态是它**声明**的 ——
    ".uking\\board\\board.json":
        j({"version": 1, "tasks": {
            "t1": {"id": "t1", "title": "浏览器玩法", "status": "doing", "folder": "D:\\work\\zeta",
                   "progress": "Gate0 已过", "updated": "2026-08-07 17:02:48"},
            "t2": {"id": "t2", "title": "等接口", "status": "blocked"},
            "t3": {"id": "t3", "title": "待排期", "status": "todo"}}}),
}

# 期望值：跑完在机器上当场断言，不靠人肉看输出。
EXPECT = {
    "claude:c1": "把合同里的甲方改一下",
    "claude:c2": "每日巡视",
    "codex:x1": "跑一遍服务器巡检",
    "hermes:h1": "写个周报",
    "openclaw:o1": "做一张宣传海报",
    "openclaw:o2": "剪个短视频",
    "board:t1": "浏览器玩法",
}
COUNTS = {"claude": 2, "codex": 1, "hermes": 1, "openclaw": 2, "board": 3}

out = [
    "chcp 65001 | Out-Null",
    "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8",
    '$ErrorActionPreference="Stop"',
    '$root="%s"' % SANDBOX,
    "Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue",
    "function WB($rel,$b64){ $p=Join-Path $root $rel; "
    "New-Item -ItemType Directory -Force -Path (Split-Path $p) | Out-Null; "
    "[IO.File]::WriteAllBytes($p,[Convert]::FromBase64String($b64)) }",
]
for rel, content in FILES.items():
    out.append('WB "%s" "%s"' % (rel, base64.b64encode(content.encode("utf-8")).decode("ascii")))

out += [
    '$env:UKING_TEST_HOME=$root',
    '$o="C:\\uk\\b2.json"',
    '$p = Start-Process -FilePath "%s" -ArgumentList (\'action run runtime.ai_tasks.inspect '
    '--json --input "{\\"days\\":365}"\') -NoNewWindow -Wait -PassThru '
    '-RedirectStandardOutput $o -RedirectStandardError "C:\\uk\\b2.err"' % EXE,
    'Write-Output ("exit=" + $p.ExitCode)',
    '$b = Get-Content $o -Raw -Encoding UTF8 | ConvertFrom-Json',
    'Write-Output ("total=" + $b.counts.total)',
    'foreach ($s in $b.sources) { Write-Output ("  {0,-9} tasks={1,-3} window={2}" '
    '-f $s.tool,$s.tasks,$s.files_in_window) }',
    'Write-Output "--- CARDS ---"',
    'foreach ($t in $b.tasks) { Write-Output ("  [{0,-8}] {1,-13} {2,-9} id={3,-14} | {4}" '
    '-f $t.tool,$t.status,$t.status_from,$t.id,$t.title) }',
    'Write-Output "--- ASSERTIONS ---"',
    '$bad=0',
]
for tool, n in COUNTS.items():
    out.append(
        '$n=@($b.tasks | Where-Object {{ $_.tool -eq "{t}" }}).Count; '
        'if ($n -ne {n}) {{ Write-Output ("FAIL {t} count=" + $n + " want={n}"); $bad++ }}'.format(t=tool, n=n))
for tid, title in EXPECT.items():
    out.append(
        '$x=@($b.tasks | Where-Object {{ $_.id -eq "{i}" }})[0]; '
        'if (-not $x) {{ Write-Output ("FAIL missing {i}"); $bad++ }} '
        'elseif ($x.title -ne "{t}") {{ Write-Output ("FAIL {i} title=" + $x.title); $bad++ }}'.format(i=tid, t=title))
out += [
    # 外部会话永不进「出错」列
    '$e=@($b.tasks | Where-Object { $_.status -eq "error" }).Count; '
    'if ($e -ne 0) { Write-Output ("FAIL 外部会话出现了 error 状态: " + $e); $bad++ }',
    # 看板 blocked 归「等待输入」，不是出错
    '$t2=@($b.tasks | Where-Object { $_.id -eq "board:t2" })[0]; '
    'if ($t2.status -ne "waiting_input") { Write-Output ("FAIL board:t2 status=" + $t2.status); $bad++ }',
    # id 唯一（撞了在界面上是 React key 撞车，不是一条报错）
    '$ids=@($b.tasks | ForEach-Object { $_.id }); '
    'if ($ids.Count -ne ($ids | Sort-Object -Unique).Count) { Write-Output "FAIL 有重复 id"; $bad++ }',
    'if ($bad -eq 0) { Write-Output "ALL ASSERTIONS PASSED" } '
    'else { Write-Output ("FAILURES=" + $bad) }',
    '$env:UKING_TEST_HOME=$null',
]
print("\n".join(out))

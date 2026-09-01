---
name: uking-teamwork
description: 在这台用 U-King 装好的电脑上，把一件大事拆开、调用本机其它 AI 命令行（Claude Code / Codex / OpenClaw / Hermes）分工协作再汇总。它们已用同一个虾盘云 Key 配好，可直接互相调用。
---

# U-King 多 AI 协同（teamwork）

这台电脑用 **U-King** 装好了多个 AI 命令行工具，而且**一键配好了同一个虾盘云 Key**。
所以你（当前 AI）不必一个人扛——可以把一件大事拆开，**调用其它 AI 命令行**分工，再把结果汇总。

## 你的队友（都已装好、同一个 Key、可直接调用）

| 队友 | 最擅长 | 一句话调用（headless / 非交互） |
|---|---|---|
| **Claude Code** | 深度推理、研究、写作、读懂大段代码、做规划、当总控 | `claude -p "<任务>"` |
| **Codex** | 自动写代码并运行、改文件、跑命令、调试 | `codex exec "<任务>"` |
| **OpenClaw**（龙虾 / ClawX） | 自动化、定时任务、多渠道（微信 / TG / 飞书等）、技能执行 | `openclaw agent --local -m "<任务>"` |
| **Hermes** | 轻量快问快答、换个模型拿「第二意见」 | `hermes -z "<任务>"` |

> **首选用本技能包里的 `scripts/call-agent.mjs` 调用**——它带**超时、输出捕获、失败兜底、防嵌套死循环**，比直接拼命令稳得多：
>
> ```bash
> node <本技能包绝对路径>/scripts/call-agent.mjs --agent codex --prompt "写一个快速排序并自测" --timeout 180
> # 加 --json 拿结构化结果：{ ok, agent, ms, output, error }
> # 加 --model <id> 指定模型
> ```
>
> 本技能包通常在：`~/.uking/skills/uking-teamwork/`（Windows 是 `%USERPROFILE%\.uking\skills\uking-teamwork\`）。
> 也可能被装进了各 AI 自己的 skills 目录（`~/.claude/skills/`、`~/.openclaw/skills/`、Hermes 的 `skills/`）。

## 怎么分工（推荐套路）

1. **总控 + 委派（首选）**：你当总控，把子任务用「一句话 + 完整背景」发给最合适的队友，拿回结果继续。
   例：你出整体方案 → 让 `codex` 写并跑代码 → 让 `hermes` 换个模型复核结论 → 你汇总。
2. **并行分工**：把几块**互相独立**的子任务同时发给不同队友（各开一个 `call-agent`），都回来后你合并。
3. **交叉验证**：同一个问题发给两个不同队友，比对分歧点，降低单一模型幻觉。

## 铁律（照做，否则会翻车）

- **只设一个总控**（就是你）。**别让队友再去调队友**形成网状——容易死循环、烧爆 token。
  `call-agent.mjs` 默认拦截嵌套调用（靠环境变量 `UKING_TEAMWORK_DEPTH`），子 agent 里再调会被拒。
- **子调用是「无记忆」的**：队友看不到你和用户的对话。把它需要的**背景、输入、期望输出格式全写进 prompt**。
- **永远带超时**（默认 180s）。网络 / 子进程可能卡死，超时会被强杀并返回失败，别裸调。
- **省着用**：每次子调用都真实扣同一个虾盘云 Key 的额度，多 agent = 成倍烧钱。**能一个人干完就别拆。**
- **要队友改文件 / 跑危险命令**：直接用对应 CLI 的自主模式（如 `codex exec --full-auto`）并由你把关，
  别用本包的简单封装（它面向「问一句、拿回文本」，不替你授权高风险操作）。

## 完整例子

任务：「做个能跑的 Python 脚本，统计某目录下代码行数，并验证正确」。

1. 你（总控）定方案 + 验收标准（统计 .py/.js、跳过空行、给出样例验证）。
2. 委派给 Codex（它最会写+跑代码）：
   ```bash
   node ~/.uking/skills/uking-teamwork/scripts/call-agent.mjs \
     --agent codex --timeout 240 \
     --prompt "写 count_lines.py：统计入参目录下 .py/.js 的非空代码行数；写完自己造一个含 3 个文件的样例目录跑一遍证明结果正确；把完整代码和运行输出都打印出来。"
   ```
3. 你审 Codex 的产出；不放心就让另一个模型挑毛病：
   ```bash
   node ~/.uking/skills/uking-teamwork/scripts/call-agent.mjs --agent hermes \
     --prompt "审查这段 Python 统计代码有没有边界 bug（贴上代码）：……"
   ```
4. 你汇总，交付给用户。

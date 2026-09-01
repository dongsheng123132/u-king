# U-King 多 AI 协同技能包（uking-teamwork）

让你装好的 AI（Claude Code / Codex / OpenClaw / Hermes）**互相调用、分工干活**。
它们已被 U-King 用同一个虾盘云 Key 配好，可直接彼此调用——一个当总控，其余当帮手。

## 怎么用

把这段话发给任意一个 AI（推荐 Claude Code，它最会当总控）：

> 「这台电脑装了多个 AI 命令行（claude / codex / openclaw / hermes），都配好了同一个 Key。
> 复杂任务请参考技能 `uking-teamwork` 把活拆开，用 `call-agent.mjs` 调其它 AI 分工，再汇总。」

AI 会自动发现本技能并按 `SKILL.md` 的套路协同。

## 直接命令行调用（也行）

```bash
# 让 Codex 写并跑一段代码
node call-agent.mjs --agent codex --prompt "写快速排序并自测" --timeout 180

# 换 Hermes 拿第二意见（结构化输出）
node call-agent.mjs --agent hermes --prompt "审一下这段逻辑有没有 bug：……" --json
```

`--agent`：`claude` | `codex` | `openclaw` | `hermes`
`--timeout`：秒，默认 180
`--model`：可选，指定模型 id
`--json`：输出 `{ ok, agent, ms, output, error }`

## 注意

- 每次调用都真实扣同一个虾盘云 Key 的额度，**多 AI 协同 = 成倍烧钱**，按需用。
- 子调用「无记忆」，背景要写全；脚本默认带超时、防嵌套死循环。
- 文件内**不含任何 Key**，各 AI 运行时自己读 `~/.uking/device.json`，文件夹可随意分发。

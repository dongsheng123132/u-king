/** Browser binding checks with fake Tauri calls; native PTY behavior is checked separately. */
import assert from 'node:assert/strict';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
const url = process.env.UKING_DEV_URL || 'http://localhost:1430/';
const folder = 'C:/Users/demo/中文 项目 100% & !';
const tools = [
  { id: 'claude-code', name: 'Claude Code CLI', installed: true, launch_cmd: 'claude', launch_app: '', config_target: 'claude', kind: 'deep' },
  { id: 'hermes', name: 'Hermes Agent', installed: true, launch_cmd: 'hermes', launch_app: '', config_target: 'hermes', kind: 'deep' },
  { id: 'claude-app', name: 'Claude 桌面版', installed: true, launch_cmd: '', launch_app: 'claude-app', config_target: null, kind: 'standalone' },
  { id: 'kimi-code', name: 'Kimi Code', installed: false, launch_cmd: 'kimi', launch_app: '', config_target: null, kind: 'standalone' },
  { id: 'grok-build', name: 'Grok Build', installed: false, launch_cmd: 'grok', launch_app: '', config_target: null, kind: 'standalone' },
  { id: 'mimo-code', name: 'MiMo Code', installed: false, launch_cmd: 'mimo', launch_app: '', config_target: null, kind: 'standalone', target: 'https://mimo.mi.com/docs/zh-CN/tokenplan/integration/mimo-code' },
  { id: 'codebuddy-code', name: 'CodeBuddy Code', installed: false, launch_cmd: 'codebuddy', launch_app: '', config_target: null, kind: 'standalone' },
  { id: 'qoder-cn', name: 'Qoder CN CLI', installed: false, launch_cmd: 'qodercn', launch_app: '', config_target: null, kind: 'standalone' },
  ...['muse-code', 'antigravity-cli', 'obsidian', 'clawx'].map(id => ({ id, name: id, installed: false, launch_cmd: '', launch_app: id, config_target: null, kind: 'standalone' })),
].map(t => ({ action: 'install', target: '', ...t, launch_mode: t.launch_app ? 'gui_app' : 'embedded_pty', hidden: false, summary: t.name }));
const browser = await chromium.launch();
const errors = [];
let page;
try {
  page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  page.setDefaultTimeout(30000);
  page.setDefaultNavigationTimeout(30000);
  page.on('pageerror', e => errors.push(String(e)));
  await page.addInitScript(({ tools, folder }) => {
    localStorage.setItem('uking.seenGuide', '1');
    localStorage.setItem('uking.launchIn', 'ucli');
    localStorage.setItem('uking.launchFolders', JSON.stringify({ v: 1, items: [{ path: folder, usedAt: Date.now() }], lastByTool: { hermes: folder } }));
    window.testCalls = [];
    const probe = { found: true, version: '1.2.3' };
    const driver = { active: { claude: 'official', hermes: 'xiapan' }, claude_own_key: true, claude_model: null, codex_model: null, hermes_model: 'demo-model', discovered: [], clawx_installed: false };
    const invoke = async (cmd, args) => {
      window.testCalls.push({ cmd, args });
      switch (cmd) {
        case 'list_tools': return tools;
        case 'get_env': return { platform: 'windows', home_dir: 'C:/Users/demo', running_from_local: true, opened_dir: null };
        case 'get_driver_status': return driver;
        case 'get_device_key': return { key: 'sk-abc123', charged: true, balance: { text: 'demo' }, recharge_url: 'https://example.com/recharge' };
        case 'get_setup_state': return { has_tool: true, has_driver: true, charged: true, next_step: '', hint: '' };
        case 'check_update': return { current: '1.3.4', latest: '1.3.4', has_update: false, checked_ok: true };
        case 'instance_role': return { role: 'primary' };
        case 'produced_file_info': return { is_dir: true };
        case 'list_providers': return [];
        case 'detect_stack': return { node: probe, npm: probe, git: probe, claude: probe, codex: { found: false, version: null }, claude_desktop: true, codex_app: false, portable_node: false, system_proxy: null };
        case 'install_tool': {
          const tool = tools.find(t => t.id === args.toolId);
          if (tool) tool.installed = true;
          return { ok: true, tool: args.toolId, version: '1.2.3', attempts: 1, error: null };
        }
        case 'action_parity_call': {
          const id = args.request.action_id;
          let result = {};
          if (id === 'runtime.tool.inspect') result = { tools: tools.map(t => ({ tool_id: t.id, mode: t.launch_app ? 'gui_app' : 'embedded_pty' })) };
          if (id === 'runtime.tool.launch') {
            const tool = tools.find(t => t.id === args.request.input.tool_id);
            result = { next: tool.launch_app ? 'done' : 'embedded_pty', launch_cmd: tool.launch_cmd, message: 'started' };
          }
          if (id === 'runtime.provider.effective') result = { targets: [] };
          return { ok: true, version: 1, action_id: id, execution_id: 'demo', result };
        }
        case 'term_open': return 'demo-pty';
        default: return cmd.startsWith('plugin:event|') ? 1 : null;
      }
    };
    window.__TAURI_INTERNALS__ = {
      invoke, convertFileSrc: p => 'https://asset.localhost/' + encodeURIComponent(p),
      transformCallback: cb => { const id = Math.floor(Math.random() * 1e9); window['_' + id] = cb; return id; },
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } }, plugins: {},
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
  }, { tools, folder });
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  await page.locator('h1', { hasText: '我的 AI' }).waitFor();
  assert.equal(await page.evaluate(() => window.testCalls.filter(c => c.args?.request?.action_id === 'runtime.tool.inspect').length), 0, 'Rendering launch buttons must not trigger a full health inspection');
  const tile = id => page.locator(`[data-testid="toolhub-tile"][data-tool-id="${id}"]`);
  // 收敛第 1 步（bd6d84231）起「可安装」区分成 推荐 / 更多 AI 工具 / 日常软件，后两组默认折叠：
  // 先展开，下面才点得到那些瓷砖（进多选后会平铺，但图标检查在多选之前）。
  for (const group of ['toolhub-group-more', 'toolhub-group-daily']) {
    const toggle = page.getByTestId(group);
    if (await toggle.count()) await toggle.click();
  }
  for (const id of ['kimi-code', 'grok-build', 'mimo-code', 'codebuddy-code', 'qoder-cn', 'muse-code', 'antigravity-cli', 'obsidian', 'clawx']) {
    const icon = tile(id).locator('[data-icon-source="brand"] img');
    await icon.waitFor();
    await icon.evaluate(img => img.decode());
    assert.ok(await icon.evaluate(img => img.complete && img.naturalWidth > 0), `${id}: brand artwork must load locally`);
  }
  await tile('claude-code').click();
  await page.getByText('官方登录', { exact: true }).first().waitFor();
  await tile('hermes').dblclick();
  await page.getByTestId('launch-folder-dialog').waitFor();
  await page.getByRole('button', { name: '在这里打开', exact: true }).click();
  await page.waitForFunction(() => window.testCalls.some(c => c.cmd === 'open_terminal_window'));
  const launch = await page.evaluate(() => window.testCalls.find(c => c.cmd === 'open_terminal_window'));
  assert.deepEqual(launch.args, { cwd: folder, cmd: 'hermes' });
  await page.evaluate(() => localStorage.setItem('uking.launchIn', 'system'));
  await tile('hermes').dblclick();
  await page.getByTestId('launch-folder-dialog').waitFor();
  await page.getByRole('button', { name: '在这里打开', exact: true }).click();
  await page.waitForFunction(() => window.testCalls.some(c => c.cmd === 'term_open_external'));
  assert.deepEqual(await page.evaluate(() => window.testCalls.find(c => c.cmd === 'term_open_external').args), { cwd: folder, cmd: 'hermes' });
  await tile('claude-app').dblclick();
  await page.waitForFunction(() => window.testCalls.some(c => c.cmd === 'action_parity_call' && c.args.request.action_id === 'runtime.tool.launch' && c.args.request.input.tool_id === 'claude-app'));
  assert.equal(await page.getByTestId('launch-folder-dialog').count(), 0, 'Desktop app should launch directly');
  await tile('mimo-code').click();
  await page.getByTestId('toolhub-config-guide').click();
  await page.waitForFunction(() => window.testCalls.some(c => c.args?.url === 'https://mimo.mi.com/docs/zh-CN/tokenplan/integration/mimo-code'));
  await page.getByTestId('toolhub-batch-toggle').click();
  await tile('mimo-code').click(); await tile('codebuddy-code').click(); await tile('qoder-cn').click();
  await page.getByTestId('toolhub-batch-install').click();
  await page.getByText('所选软件安装流程已结束。', { exact: false }).waitFor();
  const calls = await page.evaluate(() => window.testCalls);
  assert.deepEqual(calls.filter(c => c.cmd === 'install_tool').map(c => c.args.toolId), ['mimo-code', 'codebuddy-code', 'qoder-cn']);
  assert.equal(calls.filter(c => c.cmd === 'apply_provider').length, 0, 'Batch install must preserve existing providers');
  mkdirSync('shots/manager-check', { recursive: true });
  await page.screenshot({ path: 'shots/manager-check/install-result.png' });
  await page.goto(url + '?pane=terminal&cwd=' + encodeURIComponent(folder) + '&cmd=hermes', { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => window.testCalls.some(c => c.cmd === 'term_open'));
  const pty = await page.evaluate(() => window.testCalls.filter(c => c.cmd === 'term_open'));
  assert.ok(pty.some(c => c.args.cwd === folder && c.args.initialCmd === 'hermes'), 'Terminal window must pass cwd and initial command to native PTY');
  assert.equal(errors.length, 0, errors.join('\n'));
  console.log('PASS: official sign-in label, Hermes folder launch, desktop launch, ordered batch install, preserved config, terminal cwd and initial command');
} catch (error) {
  console.error('UI errors:', errors);
  console.error((await page?.textContent('body').catch(() => '') || '').slice(0, 1400));
  console.error(await page?.evaluate(() => window.testCalls?.slice(-8)).catch(() => []));
  throw error;
} finally {
  await browser.close();
}

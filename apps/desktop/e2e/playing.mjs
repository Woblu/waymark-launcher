// Checks the window closes when a game starts and comes back when it exits,
// with the crash report waiting. Run it after run.mjs, in the same data folder.
// It prints markers that a wrapper script uses to measure memory.

const PORT = 9222;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const phase = process.argv[2];

async function pageSocket(timeout = 40000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = targets.find((t) => t.type === 'page' && !t.url.startsWith('devtools'));
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(300);
  }
  return null;
}

async function open() {
  const url = await pageSocket();
  if (!url) throw new Error('no window to talk to');
  const socket = new WebSocket(url);
  await new Promise((resolve) => (socket.onopen = resolve));
  let id = 0;
  const waiting = new Map();
  socket.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id) waiting.get(data.id)?.(data);
  };
  const run = (expression) =>
    new Promise((resolve) => {
      const callId = ++id;
      waiting.set(callId, (reply) => resolve(reply.result?.result?.value));
      socket.send(JSON.stringify({ id: callId, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }));
    });
  return { socket, run };
}

if (phase === 'start') {
  const { socket, run } = await open();
  await sleep(1500);
  await run(`document.querySelector('header [aria-label="Settings"]').click()`);
  await sleep(600);
  await run(`(() => { const box = [...document.querySelectorAll('label')].find((l) => l.innerText.includes('Close the window')).querySelector('input'); if (!box.checked) box.click(); })()`);
  await sleep(500);
  await run(`[...document.querySelectorAll('.main-nav button')].find((b) => b.innerText === 'Library').click()`);
  await sleep(800);
  await run(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Play').click()`);
  console.log('clicked play');
  await new Promise((resolve) => {
    socket.onclose = resolve;
    setTimeout(resolve, 150000);
  });
  console.log(socket.readyState === WebSocket.CLOSED ? 'window closed' : 'window stayed open');
}

if (phase === 'return') {
  const { run } = await open();
  for (let i = 0; i < 60; i++) {
    await sleep(500);
    const crashed = await run(`document.body.innerText.includes('crashed')`);
    const library = await run(`document.querySelectorAll('[data-instance]').length`);
    if (library) {
      console.log(`window is back, ${library} instances, crash report showing: ${crashed}`);
      break;
    }
  }
}
process.exit(0);

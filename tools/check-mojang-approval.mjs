// Confirms whether Mojang has approved an Azure client ID for Minecraft: Java sign-in.
// Runs the real sign-in chain, then reports the verdict. Prints no tokens and saves nothing.
const clientId = process.argv[2] ?? 'aea84696-cbfe-470c-8138-023b193ce3cb';

const form = (body) => ({
  method: 'POST',
  headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams(body),
});
const json = (body) => ({
  method: 'POST',
  headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
  body: JSON.stringify(body),
});
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const base = 'https://login.microsoftonline.com/consumers/oauth2/v2.0';

const device = await (await fetch(`${base}/devicecode`, form({
  client_id: clientId,
  scope: 'XboxLive.signin offline_access',
}))).json();
if (!device.device_code) {
  console.log(`Could not start sign-in: ${device.error ?? 'unknown error'}`);
  process.exit(1);
}

console.log(`Open ${device.verification_uri} and enter this code: ${device.user_code}`);
console.log('Waiting for you to finish signing in...');

let accessToken;
const deadline = Date.now() + device.expires_in * 1000;
while (!accessToken && Date.now() < deadline) {
  await sleep(device.interval * 1000);
  const res = await (await fetch(`${base}/token`, form({
    grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
    client_id: clientId,
    device_code: device.device_code,
  }))).json();
  if (res.access_token) accessToken = res.access_token;
  else if (res.error !== 'authorization_pending' && res.error !== 'slow_down') {
    console.log(`Sign-in failed: ${res.error}`);
    process.exit(1);
  }
}
if (!accessToken) {
  console.log('Sign-in timed out. Run the script again.');
  process.exit(1);
}
console.log('Microsoft sign-in OK.');

const xbl = await fetch('https://user.auth.xboxlive.com/user/authenticate', json({
  Properties: { AuthMethod: 'RPS', SiteName: 'user.auth.xboxlive.com', RpsTicket: `d=${accessToken}` },
  RelyingParty: 'http://auth.xboxlive.com',
  TokenType: 'JWT',
}));
if (!xbl.ok) {
  console.log(`Xbox Live step failed: HTTP ${xbl.status}`);
  process.exit(1);
}
const xblData = await xbl.json();
const uhs = xblData.DisplayClaims.xui[0].uhs;
console.log('Xbox Live OK.');

const xsts = await fetch('https://xsts.auth.xboxlive.com/xsts/authorize', json({
  Properties: { SandboxId: 'RETAIL', UserTokens: [xblData.Token] },
  RelyingParty: 'rp://api.minecraftservices.com/',
  TokenType: 'JWT',
}));
if (!xsts.ok) {
  const detail = await xsts.json().catch(() => ({}));
  console.log(`XSTS step failed: HTTP ${xsts.status}, XErr=${detail.XErr ?? 'unknown'}`);
  process.exit(1);
}
const xstsData = await xsts.json();
console.log('XSTS OK.');

const mc = await fetch('https://api.minecraftservices.com/authentication/login_with_xbox', json({
  identityToken: `XBL3.0 x=${uhs};${xstsData.Token}`,
}));
const text = await mc.text();

if (mc.ok) {
  console.log(`RESULT: APPROVED. Minecraft login returned HTTP ${mc.status} for client ID ${clientId}.`);
} else if (/invalid app registration/i.test(text)) {
  console.log(`RESULT: NOT APPROVED. Minecraft login returned HTTP ${mc.status}, "Invalid app registration".`);
  console.log('Submit the client ID for review at https://aka.ms/mce-reviewappid');
} else {
  console.log(`RESULT: UNCLEAR. Minecraft login returned HTTP ${mc.status}: ${text.slice(0, 200)}`);
}

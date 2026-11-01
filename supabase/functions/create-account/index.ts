// Makes a Waymark account from a username and a password. Accounts have no real
// email address, so the sign-up has to happen here with the service key, which
// never leaves the server.
import { createClient } from 'npm:@supabase/supabase-js@2.45.4';

const USERNAME = /^[A-Za-z0-9_]{3,16}$/;
const RESERVED = ['admin', 'administrator', 'mojang', 'microsoft', 'minecraft', 'moderator', 'staff', 'support', 'system', 'waymark'];

const reply = (status: number, error?: string) =>
  new Response(JSON.stringify(error ? { error } : { ok: true }), { status, headers: { 'content-type': 'application/json' } });

async function hashed(address: string): Promise<string> {
  const salted = new TextEncoder().encode(`waymark-signup:${address}`);
  const digest = await crypto.subtle.digest('SHA-256', salted);
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

Deno.serve(async (request) => {
  if (request.method !== 'POST') return reply(405, 'method_not_allowed');
  const body = await request.json().catch(() => null);
  const username = typeof body?.username === 'string' ? body.username : '';
  const password = typeof body?.password === 'string' ? body.password : '';
  if (!USERNAME.test(username)) return reply(400, 'bad_username');
  if (RESERVED.some((name) => username.toLowerCase().includes(name))) return reply(409, 'username_taken');
  if (password.length < 8 || password.length > 72) return reply(400, 'bad_password');

  const admin = createClient(Deno.env.get('SUPABASE_URL')!, Deno.env.get('SUPABASE_SERVICE_ROLE_KEY')!);
  const address = request.headers.get('x-forwarded-for')?.split(',')[0]?.trim() ?? 'unknown';
  const { data: allowed, error: limited } = await admin.rpc('may_sign_up', { address_hash: await hashed(address) });
  if (limited) return reply(500, 'unavailable');
  if (!allowed) return reply(429, 'too_many_signups');

  const { error } = await admin.auth.admin.createUser({
    email: `${username.toLowerCase()}@users.waymark.invalid`,
    password,
    email_confirm: true,
    user_metadata: { username },
  });
  if (!error) return reply(200);
  const taken = error.status === 422 || /already|exists|duplicate/i.test(error.message);
  return reply(taken ? 409 : 500, taken ? 'username_taken' : 'unavailable');
});

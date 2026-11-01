-- Waymark accounts and friends.
--
-- Row level security is on for every table and no table has a policy, so the
-- app can't touch them directly. It can only call the functions at the bottom,
-- each of which acts as the signed-in player and nobody else.

create extension if not exists citext with schema extensions;

create table public.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  username extensions.citext not null unique check (username ~ '^[A-Za-z0-9_]{3,16}$'),
  created_at timestamptz not null default now(),
  last_seen timestamptz,
  playing text check (char_length(playing) <= 80),
  show_status boolean not null default true
);

create table public.friendships (
  id uuid primary key default gen_random_uuid(),
  requester uuid not null references public.profiles (id) on delete cascade,
  addressee uuid not null references public.profiles (id) on delete cascade,
  accepted boolean not null default false,
  created_at timestamptz not null default now(),
  check (requester <> addressee)
);
create unique index friendships_pair on public.friendships (least(requester, addressee), greatest(requester, addressee));
create index friendships_addressee on public.friendships (addressee);

create table public.blocks (
  blocker uuid not null references public.profiles (id) on delete cascade,
  blocked uuid not null references public.profiles (id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (blocker, blocked),
  check (blocker <> blocked)
);
create index blocks_blocked on public.blocks (blocked);

-- An instance one player sent another, as a share code. Codes only hold store ids.
create table public.shares (
  id uuid primary key default gen_random_uuid(),
  sender uuid not null references public.profiles (id) on delete cascade,
  recipient uuid not null references public.profiles (id) on delete cascade,
  name text not null check (char_length(name) between 1 and 80),
  code text not null check (char_length(code) <= 200000),
  created_at timestamptz not null default now()
);
create index shares_recipient on public.shares (recipient);
create index shares_sender on public.shares (sender, created_at);

-- Hashed addresses, kept for a day to slow down mass sign-ups.
create table public.signup_attempts (
  address text not null,
  created_at timestamptz not null default now()
);
create index signup_attempts_address on public.signup_attempts (address, created_at);

alter table public.profiles enable row level security;
alter table public.friendships enable row level security;
alter table public.blocks enable row level security;
alter table public.shares enable row level security;
alter table public.signup_attempts enable row level security;

-- An account's username lives in its sign-up details, so the profile follows it.
create function public.create_profile() returns trigger
language plpgsql security definer set search_path = ''
as $$
begin
  insert into public.profiles (id, username) values (new.id, new.raw_user_meta_data ->> 'username');
  return new;
end;
$$;
revoke execute on function public.create_profile() from public, anon, authenticated;

create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function public.create_profile();

create function public.may_sign_up(address_hash text) returns boolean
language plpgsql security definer set search_path = ''
as $$
declare
  recent integer;
begin
  delete from public.signup_attempts where created_at < now() - interval '1 day';
  select count(*) into recent from public.signup_attempts
    where address = address_hash and created_at > now() - interval '1 hour';
  if recent >= 5 then
    return false;
  end if;
  insert into public.signup_attempts (address) values (address_hash);
  return true;
end;
$$;
revoke execute on function public.may_sign_up(text) from public, anon, authenticated;
grant execute on function public.may_sign_up(text) to service_role;

create function public.is_blocked(a uuid, b uuid) returns boolean
language sql stable security definer set search_path = ''
as $$
  select exists (
    select 1 from public.blocks
    where (blocker = a and blocked = b) or (blocker = b and blocked = a)
  );
$$;
revoke execute on function public.is_blocked(uuid, uuid) from public, anon, authenticated;

create function public.friend_overview() returns jsonb
language plpgsql stable security definer set search_path = ''
as $$
declare
  me uuid := auth.uid();
  result jsonb;
begin
  if me is null then
    raise exception 'signed_out';
  end if;
  select jsonb_build_object(
    'username', (select username::text from public.profiles where id = me),
    'showStatus', (select show_status from public.profiles where id = me),
    'friends', coalesce((
      select jsonb_agg(jsonb_build_object(
        'id', p.id,
        'username', p.username::text,
        'online', p.show_status and p.last_seen > now() - interval '3 minutes',
        'lastSeen', case when p.show_status then p.last_seen end,
        'playing', case when p.show_status and p.last_seen > now() - interval '3 minutes' then p.playing end
      ) order by p.username)
      from public.friendships f
      join public.profiles p on p.id = case when f.requester = me then f.addressee else f.requester end
      where f.accepted and me in (f.requester, f.addressee)
    ), '[]'::jsonb),
    'incoming', coalesce((
      select jsonb_agg(jsonb_build_object('id', f.id, 'username', p.username::text) order by f.created_at)
      from public.friendships f join public.profiles p on p.id = f.requester
      where f.addressee = me and not f.accepted
    ), '[]'::jsonb),
    'outgoing', coalesce((
      select jsonb_agg(jsonb_build_object('id', f.id, 'username', p.username::text) order by f.created_at)
      from public.friendships f join public.profiles p on p.id = f.addressee
      where f.requester = me and not f.accepted
    ), '[]'::jsonb),
    'blocked', coalesce((
      select jsonb_agg(jsonb_build_object('id', p.id, 'username', p.username::text) order by p.username)
      from public.blocks b join public.profiles p on p.id = b.blocked
      where b.blocker = me
    ), '[]'::jsonb),
    'shares', coalesce((
      select jsonb_agg(jsonb_build_object('id', s.id, 'name', s.name, 'from', p.username::text, 'at', s.created_at) order by s.created_at desc)
      from public.shares s join public.profiles p on p.id = s.sender
      where s.recipient = me and s.created_at > now() - interval '30 days'
    ), '[]'::jsonb)
  ) into result;
  return result;
end;
$$;

-- Asking someone who has asked you accepts them. Blocks look the same as no such player.
create function public.request_friend(target text) returns void
language plpgsql security definer set search_path = ''
as $$
declare
  me uuid := auth.uid();
  other uuid;
  existing public.friendships%rowtype;
begin
  if me is null then
    raise exception 'signed_out';
  end if;
  select id into other from public.profiles where username = target::extensions.citext;
  if other is null or other = me or public.is_blocked(me, other) then
    raise exception 'no_such_player';
  end if;
  select * into existing from public.friendships
    where (requester = me and addressee = other) or (requester = other and addressee = me);
  if found then
    if existing.addressee = me and not existing.accepted then
      update public.friendships set accepted = true where id = existing.id;
    end if;
    return;
  end if;
  if (select count(*) from public.friendships where requester = me and not accepted) >= 50 then
    raise exception 'too_many_requests';
  end if;
  insert into public.friendships (requester, addressee) values (me, other);
end;
$$;

create function public.respond_friend(request uuid, accept boolean) returns void
language plpgsql security definer set search_path = ''
as $$
begin
  if accept then
    update public.friendships set accepted = true where id = request and addressee = auth.uid();
  else
    delete from public.friendships where id = request and auth.uid() in (requester, addressee) and not accepted;
  end if;
end;
$$;

create function public.remove_friend(other uuid) returns void
language sql security definer set search_path = ''
as $$
  delete from public.friendships
  where (requester = auth.uid() and addressee = other) or (requester = other and addressee = auth.uid());
$$;

create function public.block_player(other uuid) returns void
language plpgsql security definer set search_path = ''
as $$
declare
  me uuid := auth.uid();
begin
  if me is null or me = other then
    return;
  end if;
  delete from public.friendships where (requester = me and addressee = other) or (requester = other and addressee = me);
  delete from public.shares where (sender = other and recipient = me) or (sender = me and recipient = other);
  insert into public.blocks (blocker, blocked) values (me, other) on conflict do nothing;
end;
$$;

create function public.unblock_player(other uuid) returns void
language sql security definer set search_path = ''
as $$
  delete from public.blocks where blocker = auth.uid() and blocked = other;
$$;

create function public.heartbeat(now_playing text) returns void
language sql security definer set search_path = ''
as $$
  update public.profiles set last_seen = now(), playing = left(now_playing, 80) where id = auth.uid();
$$;

create function public.set_show_status(shown boolean) returns void
language sql security definer set search_path = ''
as $$
  update public.profiles set show_status = shown where id = auth.uid();
$$;

create function public.send_share(friend uuid, title text, share_code text) returns void
language plpgsql security definer set search_path = ''
as $$
declare
  me uuid := auth.uid();
begin
  if me is null then
    raise exception 'signed_out';
  end if;
  if not exists (
    select 1 from public.friendships
    where accepted and ((requester = me and addressee = friend) or (requester = friend and addressee = me))
  ) then
    raise exception 'not_friends';
  end if;
  if (select count(*) from public.shares where sender = me and created_at > now() - interval '1 day') >= 30 then
    raise exception 'too_many_shares';
  end if;
  delete from public.shares where created_at < now() - interval '30 days';
  insert into public.shares (sender, recipient, name, code) values (me, friend, left(title, 80), share_code);
end;
$$;

create function public.share_code(share uuid) returns text
language sql stable security definer set search_path = ''
as $$
  select code from public.shares where id = share and recipient = auth.uid();
$$;

create function public.drop_share(share uuid) returns void
language sql security definer set search_path = ''
as $$
  delete from public.shares where id = share and recipient = auth.uid();
$$;

create function public.delete_account() returns void
language sql security definer set search_path = ''
as $$
  delete from auth.users where id = auth.uid();
$$;

-- Only signed-in players may call any of these.
do $$
declare
  name text;
begin
  foreach name in array array[
    'friend_overview()', 'request_friend(text)', 'respond_friend(uuid, boolean)', 'remove_friend(uuid)',
    'block_player(uuid)', 'unblock_player(uuid)', 'heartbeat(text)', 'set_show_status(boolean)',
    'send_share(uuid, text, text)', 'share_code(uuid)', 'drop_share(uuid)', 'delete_account()'
  ] loop
    execute format('revoke execute on function public.%s from public, anon', name);
    execute format('grant execute on function public.%s to authenticated', name);
  end loop;
end;
$$;

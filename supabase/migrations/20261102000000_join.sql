-- Lets friends join a server a player hosts from Waymark. A host's address is only
-- ever shown to their friends, and only while the host is online and showing their
-- status. Nothing here changes what older copies of the app call.

alter table public.profiles
  add column hosting_title text check (char_length(hosting_title) <= 80),
  add column hosting_address text check (char_length(hosting_address) <= 128),
  add column hosting_code text check (char_length(hosting_code) <= 200000);

create or replace function public.friend_overview() returns jsonb
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
        'playing', case when p.show_status and p.last_seen > now() - interval '3 minutes' then p.playing end,
        'hosting', case when p.show_status and p.last_seen > now() - interval '3 minutes' and p.hosting_address is not null
          then jsonb_build_object('title', p.hosting_title, 'address', p.hosting_address) end
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

-- Everything null means the player isn't hosting any more.
create function public.set_hosting(title text, address text, code text) returns void
language sql security definer set search_path = ''
as $$
  update public.profiles
  set hosting_title = left(title, 80), hosting_address = left(address, 128), hosting_code = code
  where id = auth.uid();
$$;

-- The instance a friend is hosting. Only that friend's friends can have it.
create function public.hosting_code(friend uuid) returns text
language sql stable security definer set search_path = ''
as $$
  select p.hosting_code
  from public.friendships f
  join public.profiles p on p.id = friend
  where f.accepted
    and auth.uid() in (f.requester, f.addressee)
    and friend in (f.requester, f.addressee)
    and friend <> auth.uid()
    and p.show_status
    and p.last_seen > now() - interval '3 minutes';
$$;

revoke execute on function public.set_hosting(text, text, text) from public, anon;
grant execute on function public.set_hosting(text, text, text) to authenticated;
revoke execute on function public.hosting_code(uuid) from public, anon;
grant execute on function public.hosting_code(uuid) to authenticated;

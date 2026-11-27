# Menagerie agent for Home Assistant OS

Home Assistant OS has a read-only system and no place for your own services; add-ons (now called
apps) are how anything long-lived runs there. This app builds the agent from source on the machine and runs
it on the host's network, listening on port 9184.

## Install

Copy this folder into the machine's local apps, then install it from the app store in Settings
(it is listed under local apps after ⋮ → Check for updates), or with the `ha` CLI:

```sh
scp -r deploy/home-assistant/menagerie_agent root@homeassistant:/mnt/data/supervisor/addons/local/  # port 22222 is the host's debug SSH
ha store reload
ha apps install local_menagerie_agent   # builds the agent: a few minutes on a Raspberry Pi
ha apps start local_menagerie_agent
```

Then point the machine's fleet entry at it: `agent: http://homeassistant:9184`.

## What it reports

Load, memory, temperature and uptime are the machine's own. The disk is the data partition,
where the app's `/data` lives (`MENAGERIE_AGENT_DISK=/data`). There is no Nix configuration,
so there is no revision and nothing to sync.

The agent listens on every interface of the host, so anything on the LAN or tailnet can read
`/status` and `/metrics`. Set `listen` to the machine's Tailscale address (100.x.y.z:9184) to
keep it on the tailnet.

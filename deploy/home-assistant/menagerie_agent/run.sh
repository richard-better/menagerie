#!/usr/bin/with-contenv bashio
# The add-on's /data lives on the machine's data partition, so its usage is the disk's.
export MENAGERIE_AGENT_DISK=/data
# The container's hostname is the app's slug; report the machine's instead.
MENAGERIE_AGENT_HOSTNAME=$(bashio::host.hostname)
export MENAGERIE_AGENT_HOSTNAME
exec /usr/bin/menagerie-agent --listen "$(bashio::config 'listen')"

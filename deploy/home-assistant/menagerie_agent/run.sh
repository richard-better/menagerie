#!/usr/bin/with-contenv bashio
# The add-on's /data lives on the machine's data partition, so its usage is the disk's.
export MENAGERIE_AGENT_DISK=/data
exec /usr/bin/menagerie-agent --listen "$(bashio::config 'listen')"

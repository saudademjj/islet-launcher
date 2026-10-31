#!/bin/sh
# Keeps this Minecraft server running. Written by the Islet launcher; it
# replaces this file on every upload.
#
#   sh islet.sh start | stop | restart | status
#   sh islet.sh cmd TEXT      type TEXT into the server console
#   sh islet.sh log [LINES]   the end of the console log
#   sh islet.sh backup        a copy of the world in backups/
#   sh islet.sh set-prop KEY VALUE | memory MB | schedule AUTOSTART BACKUP
cd "$(dirname "$0")" || exit 1
HERE=$(pwd)
mkdir -p .islet logs backups
PID=.islet/server.pid
IN=.islet/console.in
LOG=logs/islet-console.log
KEEP=$(cat .islet/keep 2>/dev/null || echo 7)

running() { [ -f "$PID" ] && kill -0 "$(cat "$PID")" 2>/dev/null; }

# The console reads a named pipe that the server's own process holds open
# for writing too, so it never sees the end of its input.
send() { running && [ -p "$IN" ] && printf '%s\n' "$1" > "$IN"; }

level() {
	name=$(sed -n 's/^level-name=//p' server.properties 2>/dev/null | tail -n 1)
	echo "${name:-world}"
}

start() {
	if running; then echo running; return 0; fi
	[ -f start.sh ] || { echo "start.sh is missing" >&2; return 1; }
	rm -f "$IN"
	mkfifo "$IN" || return 1
	[ -x java/bin/java ] && JAVA="$HERE/java/bin/java" && export JAVA
	[ -f "$LOG" ] && mv -f "$LOG" "$LOG.1"
	setsid sh -c 'exec 3<>"$1"; sh start.sh <&3 >>"$2" 2>&1' islet "$IN" "$LOG" </dev/null >/dev/null 2>&1 &
	echo $! > "$PID"
	echo started
}

stop() {
	if ! running; then rm -f "$PID"; echo stopped; return 0; fi
	send stop
	i=0
	while running && [ $i -lt 90 ]; do sleep 1; i=$((i + 1)); done
	if running; then
		kill -TERM -- "-$(cat "$PID")" 2>/dev/null
		sleep 5
		kill -KILL -- "-$(cat "$PID")" 2>/dev/null
	fi
	rm -f "$PID" "$IN"
	echo stopped
}

saved() { n=$(grep -cE 'Saved the (game|world)' "$LOG" 2>/dev/null); echo "${n:-0}"; }

backup() {
	world=$(level)
	[ -d "$world" ] || { echo "no world yet" >&2; return 1; }
	name="backups/world-$(date +%Y%m%d-%H%M%S).tar.gz"
	if running; then
		before=$(saved)
		send save-off
		send "save-all flush"
		i=0
		while [ $i -lt 120 ] && [ "$(saved)" -le "$before" ]; do sleep 1; i=$((i + 1)); done
	fi
	tar -czf "$name.part" "$world"
	ok=$?
	running && send save-on
	if [ $ok -ne 0 ]; then rm -f "$name.part"; return 1; fi
	mv -f "$name.part" "$name"
	ls -1t backups/world-*.tar.gz 2>/dev/null | tail -n +$((KEEP + 1)) | while read -r old; do rm -f "$old"; done
	echo "$name"
}

status() {
	if running; then echo state=running; else echo state=stopped; fi
	if running && grep -q 'Done (' "$LOG" 2>/dev/null; then echo ready=1; else echo ready=0; fi
	for f in $(ls -1t backups/world-*.tar.gz 2>/dev/null); do echo "backup=$(wc -c < "$f" | tr -d ' ') $f"; done
	echo "schedule=$(cat .islet/schedule 2>/dev/null)"
}

set_prop() {
	touch server.properties
	if grep -q "^$1=" server.properties; then
		sed -i "s|^$1=.*|$1=$2|" server.properties
	else
		echo "$1=$2" >> server.properties
	fi
}

memory() {
	for f in start.sh user_jvm_args.txt; do
		[ -f "$f" ] && sed -i "s/-Xms[0-9]*[MmGg]/-Xms$1M/; s/-Xmx[0-9]*[MmGg]/-Xmx$1M/" "$f"
	done
	true
}

# Starting at boot and a backup every night: cron when there is one, else
# systemd units when running as root.
schedule() {
	tag="# islet:$HERE"
	unit="islet-$(printf %s "$HERE" | cksum | cut -d ' ' -f 1)"
	if command -v crontab >/dev/null 2>&1; then
		{
			crontab -l 2>/dev/null | grep -vF "$tag"
			[ "$1" = 1 ] && echo "@reboot sh $HERE/islet.sh start >/dev/null 2>&1 $tag"
			[ "$2" = 1 ] && echo "17 5 * * * sh $HERE/islet.sh backup >/dev/null 2>&1 $tag"
		} | crontab - && echo cron > .islet/schedule
	elif [ "$(id -u)" = 0 ] && command -v systemctl >/dev/null 2>&1; then
		systemd=/etc/systemd/system
		rm -f "$systemd/$unit.service" "$systemd/$unit-backup.service" "$systemd/$unit-backup.timer"
		if [ "$1" = 1 ]; then
			printf '[Unit]\nDescription=Minecraft server in %s\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/sh %s/islet.sh start\nExecStop=/bin/sh %s/islet.sh stop\n\n[Install]\nWantedBy=multi-user.target\n' "$HERE" "$HERE" "$HERE" > "$systemd/$unit.service"
		fi
		if [ "$2" = 1 ]; then
			printf '[Unit]\nDescription=Back up the Minecraft world in %s\n\n[Service]\nType=oneshot\nExecStart=/bin/sh %s/islet.sh backup\n' "$HERE" "$HERE" > "$systemd/$unit-backup.service"
			printf '[Unit]\nDescription=Nightly backup of the Minecraft world in %s\n\n[Timer]\nOnCalendar=*-*-* 05:17\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n' "$HERE" > "$systemd/$unit-backup.timer"
		fi
		systemctl daemon-reload
		[ "$1" = 1 ] && systemctl enable "$unit.service" >/dev/null 2>&1
		[ "$2" = 1 ] && systemctl enable --now "$unit-backup.timer" >/dev/null 2>&1
		echo systemd > .islet/schedule
	else
		echo none > .islet/schedule
	fi
	cat .islet/schedule
}

case "$1" in
	start) start ;;
	stop) stop ;;
	restart) stop >/dev/null && start ;;
	status) status ;;
	cmd) running || { echo "the server isn't running" >&2; exit 1; }; send "$2" ;;
	log) tail -n "${2:-200}" "$LOG" 2>/dev/null; true ;;
	backup) backup ;;
	set-prop) set_prop "$2" "$3" ;;
	memory) memory "$2" ;;
	schedule) schedule "$2" "$3" ;;
	*) sed -n '2,9p' "$0"; exit 2 ;;
esac

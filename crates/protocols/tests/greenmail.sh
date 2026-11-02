#!/usr/bin/env bash
# Starts/stops the GreenMail container used by tests/greenmail.rs.
#
#   crates/protocols/tests/greenmail.sh start   # then:
#   BGMAIL_GREENMAIL=1 cargo test -p protocols --test greenmail -- --ignored
#   crates/protocols/tests/greenmail.sh stop
#
# Optional Dovecot (CONDSTORE/MOVE/STARTTLS coverage, IMAP only; any user,
# password "pass"; IMAP+STARTTLS 31143, IMAPS 31993):
#   crates/protocols/tests/greenmail.sh dovecot
#   BGMAIL_DOVECOT=1 cargo test -p protocols --test greenmail -- --ignored
#
# Ports (GreenMail "test" setup): SMTP 3025, SMTPS 3465, IMAP 3143,
# IMAPS 3993 (self-signed cert), POP3 3110, POP3S 3995.
# Users: alice / bob, password "secret", login with the full address.
set -euo pipefail

NAME=bgmail-greenmail
IMAGE=greenmail/standalone:2.1.15
DOVECOT_NAME=bgmail-dovecot
DOVECOT_IMAGE=dovecot/dovecot:2.4.5

case "${1:-start}" in
  start)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --rm --name "$NAME" \
      -e GREENMAIL_OPTS='-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=alice:secret@localhost,bob:secret@localhost -Dgreenmail.users.login=email -Dgreenmail.verbose' \
      -p 3025:3025 -p 3110:3110 -p 3143:3143 -p 3465:3465 -p 3993:3993 -p 3995:3995 \
      "$IMAGE" >/dev/null
    for _ in $(seq 1 60); do
      # docker-proxy accepts early; wait for the real IMAP greeting.
      if timeout 2 bash -c 'exec 3<>/dev/tcp/127.0.0.1/3143 && head -c 4 <&3' 2>/dev/null | grep -q '^\* OK'; then
        echo "GreenMail is up ($IMAGE)"
        exit 0
      fi
      sleep 0.5
    done
    echo "GreenMail did not start" >&2
    docker logs "$NAME" >&2 || true
    exit 1
    ;;
  dovecot)
    docker rm -f "$DOVECOT_NAME" >/dev/null 2>&1 || true
    docker run -d --rm --name "$DOVECOT_NAME" -e USER_PASSWORD=pass \
      -p 31143:31143 -p 31993:31993 \
      "$DOVECOT_IMAGE" >/dev/null
    for _ in $(seq 1 60); do
      if timeout 2 bash -c 'exec 3<>/dev/tcp/127.0.0.1/31143 && head -c 4 <&3' 2>/dev/null | grep -q '^\* OK'; then
        echo "Dovecot is up ($DOVECOT_IMAGE)"
        exit 0
      fi
      sleep 0.5
    done
    echo "Dovecot did not start" >&2
    exit 1
    ;;
  stop)
    docker rm -f "$NAME" "$DOVECOT_NAME" >/dev/null 2>&1 || true
    echo "Test servers stopped"
    ;;
  *)
    echo "usage: $0 [start|dovecot|stop]" >&2
    exit 2
    ;;
esac

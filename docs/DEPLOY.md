# Deploying to the Raspberry Pi

The app runs on the Pi as a systemd service, as its own `ttrpg` user, in a sandbox. It's built on
the desktop (DESIGN §8) and copied over by `scripts/deploy.sh`.

| What | Where on the Pi |
|---|---|
| Binary, `templates/`, `static/` | `/opt/ttrpg-scheduling/` (owned by your login user, so `rsync` can write it) |
| Database | `/var/lib/ttrpg-scheduling/app.db` (created by systemd, owned by `ttrpg`) |
| Nightly backups | `/var/lib/ttrpg-scheduling/backups/app-<UTC time>.db`, kept 14 days |
| Configuration | `/etc/ttrpg-scheduling.env` |
| Units | `/etc/systemd/system/ttrpg-scheduling{,-backup}.service`, `ttrpg-scheduling-backup.timer` |

Checked on the desktop on 2026-10-21:

- the aarch64 musl release binary, run under `qemu-aarch64`
- the service's sandbox settings, with `systemd-run --user`
- the units, with `systemd-analyze verify`

Not yet checked on the Pi itself.

## First-time setup (once)

On the Pi (64-bit Raspberry Pi OS, with the VPN already set up):

```sh
# A system user without a login shell or home directory
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin ttrpg

# The app directory, writable by you (deploys) and readable by the service
sudo mkdir -p /opt/ttrpg-scheduling
sudo chown "$USER": /opt/ttrpg-scheduling

# Configuration: copy deploy/ttrpg-scheduling.env.example from the repo and edit BIND_ADDR
sudo install -m 0640 -o root -g ttrpg ttrpg-scheduling.env.example /etc/ttrpg-scheduling.env
sudo nano /etc/ttrpg-scheduling.env

# The units (copy deploy/*.service and deploy/*.timer over first)
sudo cp ttrpg-scheduling.service ttrpg-scheduling-backup.service ttrpg-scheduling-backup.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable ttrpg-scheduling.service ttrpg-scheduling-backup.timer

# Keep the clock right (the Pi has no RTC; timestamps and deadlines depend on it)
timedatectl show -p NTPSynchronized
```

**`BIND_ADDR`:** use the Pi's VPN address (for Tailscale, `tailscale ip -4`) with port 8080.
The app is then unreachable from the LAN.
- If the VPN isn't up yet at boot, the bind fails and systemd retries every 5 seconds until it is.
- Binding to `0.0.0.0:8080` also works, but then a firewall has to keep the LAN out.

**The deploy script needs passwordless `sudo` for exactly three commands.** Create
`/etc/sudoers.d/ttrpg-deploy` with `sudo visudo -f /etc/sudoers.d/ttrpg-deploy`, and replace
`pi` with your login user:

```
pi ALL=(root) NOPASSWD: /usr/bin/systemctl restart ttrpg-scheduling.service, /usr/bin/systemctl start ttrpg-scheduling-backup.service
```

## Deploying

From the repo on the desktop:

```sh
PI_HOST=pi@raspberrypi HEALTH_URL=http://100.x.y.z:8080/healthz scripts/deploy.sh
```

The script does five things:
1. It builds with `cargo zigbuild --release --target aarch64-unknown-linux-musl`.
2. It runs a backup first, because a new version may run migrations on start. On the first
   deploy there is no binary on the Pi yet, so it skips this step.
3. It copies `templates/` and `static/` and the binary. The binary goes under a temporary name
   that is then renamed, so a running binary is never half-written.
4. It restarts the service. systemd sends SIGTERM, and the app finishes in-flight requests
   before it exits.
5. It checks `/healthz` (`ok <version>`).

**The first deploy** creates the database. Then create the first admin:

```sh
ssh pi@raspberrypi
sudo -u ttrpg sh -c 'set -a; . /etc/ttrpg-scheduling.env; exec /opt/ttrpg-scheduling/ttrpg-scheduling create-admin alice "Alice"'
```

Everyone else joins through campaign invite links.

## Operating

```sh
systemctl status ttrpg-scheduling              # running?
journalctl -u ttrpg-scheduling -f              # logs (RUST_LOG in the env file sets the level)
systemctl list-timers ttrpg-scheduling-backup  # next backup
sudo systemctl start ttrpg-scheduling-backup   # back up now
curl http://<BIND_ADDR>/healthz                # "ok 0.1.0+<commit>"
```

The version (`0.1.0+<git commit>`, with `-dirty` for uncommitted changes) is also in the page
footer.

## Backups

- **What the nightly timer runs:** `ttrpg-scheduling backup <file>`, which is SQLite's
  `VACUUM INTO`. It makes a consistent copy while the app keeps running, and needs no
  `sqlite3` tool on the Pi.
- **They must leave the SD card.** Copies on the Pi only protect against mistakes, not against
  a dead card. Pull them from the desktop or NAS, e.g. with a daily cron job or systemd timer
  there:

  ```sh
  rsync -a pi@raspberrypi:/var/lib/ttrpg-scheduling/backups/ ~/backups/ttrpg-scheduling/
  ```

  For that, your login user needs read access: `sudo usermod -aG ttrpg pi`, then log in again.
  The directory is `0750 ttrpg:ttrpg`.

**Restoring:**

```sh
sudo systemctl stop ttrpg-scheduling
sudo -u ttrpg cp /var/lib/ttrpg-scheduling/backups/app-<time>.db /var/lib/ttrpg-scheduling/app.db
sudo rm -f /var/lib/ttrpg-scheduling/app.db-wal /var/lib/ttrpg-scheduling/app.db-shm
sudo systemctl start ttrpg-scheduling
```

## The sandbox

The unit allows the app only what it needs:
- read `/opt/ttrpg-scheduling`
- write `/var/lib/ttrpg-scheduling`
- open TCP and Unix sockets
- the `@system-service` system calls
- at most 256 MB of memory

`systemd-analyze security ttrpg-scheduling` on the Pi rates it.

If a future feature needs more, such as outgoing HTTPS for a Discord webhook, the unit is the
place to allow it. Outgoing connections are already allowed, because `RestrictAddressFamilies`
includes `AF_INET`.

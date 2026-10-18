# imap-trial (Phase-0 spike)

This program compares `async-imap` and `imap-next` against a real Dovecot server and against a deliberately broken one. The results and the decision are in [docs/research/07-imap-library-spike.md](../../docs/research/07-imap-library-spike.md).

It is **not product code**. It has its own Cargo workspace, so its dependencies, including the imap-codec alphas, stay out of the product build and its licence checks.

## Reproduce

1. **Create a throwaway test CA and a `localhost` certificate** (any directory):

   ```sh
   openssl req -x509 -newkey rsa:2048 -nodes -days 30 -subj "/CN=Regards Test CA" -keyout ca.key -out ca.pem
   openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" -keyout server.key -out server.csr
   printf "subjectAltName=DNS:localhost,IP:127.0.0.1\n" > leaf.ext
   openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 30 -extfile leaf.ext -out server.pem
   ```

   On Git Bash for Windows, prefix the commands with `MSYS_NO_PATHCONV=1` so `-subj` isn't rewritten as a path.

2. **Start Dovecot.** The spike used `dovecot/dovecot:latest`, which was 2.4.5. Any user name logs in with the password `pass`, and each user gets its own empty mailbox.

   ```sh
   docker run -d --name regards-dovecot -e USER_PASSWORD=pass \
     -p 127.0.0.1:31993:31993 \
     -v "$PWD/server.pem:/etc/dovecot/ssl/tls.crt:ro" \
     -v "$PWD/server.key:/etc/dovecot/ssl/tls.key:ro" \
     dovecot/dovecot:latest
   ```

3. **Run the trial:**

   ```sh
   TRIAL_CA=/path/to/ca.pem cargo run -- live     # both libraries against Dovecot
   cargo run --release -- garbage 2000            # malformed-server cases + 2000 fuzz seeds per library
   ```

   `TRIAL_ADDR` (default `127.0.0.1:31993`) and `TRIAL_PASS` (default `pass`) override the defaults.

## Notes

- **Child processes.** `garbage` runs each library in a child process (`garbage-child`), so a stack overflow is recorded as a result instead of ending the run.
- **Fuzz seeds.** They are deterministic: the same seed always produces the same bytes.

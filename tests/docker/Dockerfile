# A remote host for omt's SSH tests.
#
# Deliberately minimal and deliberately *not* omt-aware: it is a plain sshd on
# musl, which is what a real target host looks like. The tests bring omt to it,
# which is the thing worth testing — an image with omt baked in would pass
# whether or not the bootstrap works.
FROM alpine:3.21

RUN apk add --no-cache openssh-server openssh-client shadow bash

# Host keys, and a user the tests log in as.
# `adduser -D` leaves `!` in shadow, which sshd reads as a *locked account* and
# refuses even with a valid key. `*` means "no password login" without locking,
# which is what key-only actually wants.
RUN ssh-keygen -A \
 && adduser -D -s /bin/sh omtuser \
 && sed -i 's/^omtuser:!/omtuser:*/' /etc/shadow \
 && mkdir -p /home/omtuser/.ssh \
 && chown -R omtuser:omtuser /home/omtuser/.ssh \
 && chmod 700 /home/omtuser/.ssh

# Key-only login. A password-enabled test host is a habit that escapes.
RUN sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config \
 && sed -i 's/^#\?PubkeyAuthentication.*/PubkeyAuthentication yes/' /etc/ssh/sshd_config \
 && sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]

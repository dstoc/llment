FROM archlinux

ARG USERNAME=user
ARG UID=1000
ARG GID=1000

RUN pacman -Syu --noconfirm --needed \
    git jujutsu \
    bash less \
    rust ripgrep \
    npm \
    jq \
    python uv \
    pkg-config openssl base-devel \
    && groupadd -g $GID $USERNAME \
    && useradd -m -u $UID -g $GID -s /bin/bash $USERNAME

RUN git clone --depth 1 https://github.com/openai/codex \
    && CARGO_INSTALL_ROOT=/usr/local cargo install --locked --path codex/codex-rs/apply-patch \
    && rm -rf codex \
    && rm -rf /root/.cargo

USER $USERNAME
WORKDIR /home/$USERNAME
CMD ["bash"]

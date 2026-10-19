#!/bin/sh
# Copia o motor do Decanta (app) para cá (motor/).
# O repositório do app é privado e a compilação no GitHub não o alcança; por isso a cópia.
#
#   scripts/atualizar-motor.sh [pasta do app Decanta]   (padrão: ./decanta)
set -e
cd "$(dirname "$0")/.."
ORIGEM="${1:-decanta}"
[ -f "$ORIGEM/motor/Cargo.toml" ] || { echo "não achei $ORIGEM/motor"; exit 1; }
rm -rf motor

cp -R "$ORIGEM/motor" motor
rm -rf motor/target motor/tests/referencia/__pycache__
# o áudio de teste do app é privado; o daqui é test-audio/gravacao-teste-decanta.wav
# o Cargo.toml do motor herda do workspace do app; aqui ele fica sozinho
sed -i.bak -e 's/^version.workspace = true/version = "0.1.0"/' \
           -e 's/^edition.workspace = true/edition = "2021"/' \
           -e 's/^license.workspace = true/license = "MIT OR Apache-2.0"/' motor/Cargo.toml
rm motor/Cargo.toml.bak
COMMIT=$(git -C "$ORIGEM" log -1 --format="%h %s" 2>/dev/null || echo "?")
echo "$COMMIT" > motor/ORIGEM.txt
echo "motor atualizado: $COMMIT"

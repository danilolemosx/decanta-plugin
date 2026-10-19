#!/bin/sh
# Monta o instalador do Mac (Decanta-<versão>-mac.pkg) a partir dos plugins compilados.
#
#   scripts/empacotar-mac.sh <pasta com Decanta.vst3, Decanta.component e o .aaxplugin> <versão>
#
# O AAX é assinado aqui com as ferramentas da PACE: precisa do iLok de assinatura conectado e do
# iLok License Manager aberto na conta de assinatura. Os dados da assinatura vêm de variáveis ou do
# arquivo local scripts/assinatura.local (fora do repositório):
#   PACE_CUSTOMER_NUMBER, PACE_CUSTOMER_NAME, SIGN_ID (certificado de assinatura do Mac)
#
# O instalador não é assinado pela Apple: na primeira vez, o macOS pede "Abrir Mesmo Assim" em
# Ajustes do Sistema → Privacidade e Segurança. O que ele instala já fica liberado (sem quarentena).
set -e
# sem arquivos "._" (metadados do macOS) dentro do pacote
export COPYFILE_DISABLE=1
cd "$(dirname "$0")/.."
ORIGEM="$1"
VERSAO="$2"
[ -d "$ORIGEM" ] && [ -n "$VERSAO" ] || { echo "uso: $0 <pasta dos plugins> <versão>"; exit 1; }

WRAPTOOL=/Applications/PACEAntiPiracy/Eden/Fusion/Versions/6/bin/wraptool
[ -f scripts/assinatura.local ] && . scripts/assinatura.local
[ -n "$PACE_CUSTOMER_NUMBER" ] && [ -n "$PACE_CUSTOMER_NAME" ] && [ -n "$SIGN_ID" ] || {
	echo "defina PACE_CUSTOMER_NUMBER, PACE_CUSTOMER_NAME e SIGN_ID (ou scripts/assinatura.local)"; exit 1; }

AAX_ORIGEM=$(ls -d "$ORIGEM"/Decanta.aaxplugin* | head -1)
for f in "$ORIGEM/Decanta.vst3" "$ORIGEM/Decanta.component" "$AAX_ORIGEM"; do
	[ -d "$f" ] || { echo "faltando: $f"; exit 1; }
done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
RAIZ="$TMP/raiz"
mkdir -p "$RAIZ/Library/Audio/Plug-Ins/VST3" "$RAIZ/Library/Audio/Plug-Ins/Components" \
         "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins"
ditto "$ORIGEM/Decanta.vst3" "$RAIZ/Library/Audio/Plug-Ins/VST3/Decanta.vst3"
ditto "$ORIGEM/Decanta.component" "$RAIZ/Library/Audio/Plug-Ins/Components/Decanta.component"

echo "== assinando VST3 e AU (certificado do Mac)"
# o JUCE grava o moduleinfo.json do VST3 depois de assinar, e o AU universal vem sem assinatura
for p in "$RAIZ/Library/Audio/Plug-Ins/VST3/Decanta.vst3" "$RAIZ/Library/Audio/Plug-Ins/Components/Decanta.component"; do
	codesign --force --sign "$SIGN_ID" --timestamp=none "$p"
	codesign --verify --strict "$p"
done

echo "== assinando o AAX (PACE)"
ditto "$AAX_ORIGEM" "$TMP/Decanta.aaxplugin"
"$WRAPTOOL" sign --customernumber "$PACE_CUSTOMER_NUMBER" --customername "$PACE_CUSTOMER_NAME" \
	--productname "Decanta" --signid "$SIGN_ID" \
	--in "$TMP/Decanta.aaxplugin" --out "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins/Decanta.aaxplugin"
"$WRAPTOOL" verify --in "$RAIZ/Library/Application Support/Avid/Audio/Plug-Ins/Decanta.aaxplugin" | grep -q "The digital signature was verified"

echo "== instalador"
xattr -cr "$RAIZ"
pkgbuild --root "$RAIZ" --identifier br.danilolemos.decanta.plugins --version "$VERSAO" \
	--install-location / "$TMP/decanta-plugins.pkg"
mkdir -p "$TMP/recursos"
cp scripts/instalador/*.html "$TMP/recursos/"
cat > "$TMP/distribution.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Decanta $VERSAO</title>
    <welcome file="bem-vindo.html" mime-type="text/html"/>
    <license file="licenca.html" mime-type="text/html"/>
    <conclusion file="concluido.html" mime-type="text/html"/>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <domains enable_localSystem="true"/>
    <choices-outline><line choice="decanta"/></choices-outline>
    <choice id="decanta" title="Decanta (VST3, AU e AAX)">
        <pkg-ref id="br.danilolemos.decanta.plugins"/>
    </choice>
    <pkg-ref id="br.danilolemos.decanta.plugins" version="$VERSAO">decanta-plugins.pkg</pkg-ref>
</installer-gui-script>
EOF
mkdir -p dist
productbuild --distribution "$TMP/distribution.xml" --resources "$TMP/recursos" --package-path "$TMP" \
	"dist/Decanta-$VERSAO-mac.pkg"
echo "pronto: dist/Decanta-$VERSAO-mac.pkg"

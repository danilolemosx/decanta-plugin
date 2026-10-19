#include "PluginEditor.h"

#include "BinaryData.h"

using namespace juce;

static const char* const idsSliders[] = { ID::voz, ID::separacao, ID::ambiencia, ID::compressao, ID::ganho,
                                          ID::threshold, ID::ratio, ID::attack, ID::release, ID::knee,
                                          ID::posFiltro, ID::limiarRuido };
static const char* const idsChaves[] = { ID::soFundo, ID::compManual };

DecantaEditor::DecantaEditor (DecantaProcessor& p)
    : AudioProcessorEditor (&p), processador (p)
{
    for (auto* id : idsSliders)
        sliders.push_back (std::make_unique<WebSliderRelay> (id));
    for (auto* id : idsChaves)
        chaves.push_back (std::make_unique<WebToggleButtonRelay> (id));

    pagina = std::make_unique<WebBrowserComponent> (opcoes());

    for (size_t i = 0; i < sliders.size(); ++i)
        ligacoesSliders.push_back (std::make_unique<WebSliderParameterAttachment> (
            *processador.state.getParameter (idsSliders[i]), *sliders[i], nullptr));
    for (size_t i = 0; i < chaves.size(); ++i)
        ligacoesChaves.push_back (std::make_unique<WebToggleButtonParameterAttachment> (
            *processador.state.getParameter (idsChaves[i]), *chaves[i], nullptr));

    addAndMakeVisible (*pagina);
    pagina->goToURL (WebBrowserComponent::getResourceProviderRoot());

    setSize (560, 440);
    startTimerHz (20);
}

DecantaEditor::~DecantaEditor()
{
    stopTimer();
}

WebBrowserComponent::Options DecantaEditor::opcoes()
{
    auto o = WebBrowserComponent::Options {}
                 .withBackend (WebBrowserComponent::Options::Backend::webview2)
                 .withWinWebView2Options (WebBrowserComponent::Options::WinWebView2 {}
                                              .withUserDataFolder (File::getSpecialLocation (File::SpecialLocationType::tempDirectory)))
                 .withNativeIntegrationEnabled()
                 .withKeepPageLoadedWhenBrowserIsHidden()
                 .withResourceProvider ([this] (const auto& url) { return recurso (url); })
                 // a página avisa que carregou; com DECANTA_TESTE_JANELA=<arquivo> isso vira um
                 // arquivo de conferência (usado pelos testes, que não podem capturar a tela)
                 .withNativeFunction ("janelaPronta", [] (const Array<var>& args, auto completar)
                 {
                     if (SystemStats::getEnvironmentVariable ("DECANTA_TESTE_JANELA", {}).isNotEmpty())
                         File (SystemStats::getEnvironmentVariable ("DECANTA_TESTE_JANELA", {}))
                             .replaceWithText (args.isEmpty() ? String() : args[0].toString());
                     completar (var());
                 });
    for (auto& s : sliders)
        o = o.withOptionsFrom (*s);
    for (auto& c : chaves)
        o = o.withOptionsFrom (*c);
    return o;
}

std::optional<WebBrowserComponent::Resource> DecantaEditor::recurso (const String& url)
{
    const auto nome = url == "/" ? String ("index.html") : url.fromFirstOccurrenceOf ("/", false, false);

    struct Arquivo { const char* url; const char* dados; int tamanho; const char* mime; };
    const Arquivo arquivos[] = {
        { "index.html", BinaryData::index_html, BinaryData::index_htmlSize, "text/html" },
        { "style.css", BinaryData::style_css, BinaryData::style_cssSize, "text/css" },
        { "app.js", BinaryData::app_js, BinaryData::app_jsSize, "text/javascript" },
        { "juce.js", BinaryData::index_js, BinaryData::index_jsSize, "text/javascript" },
        { "fonts/archivo.woff2", BinaryData::archivo_woff2, BinaryData::archivo_woff2Size, "font/woff2" },
    };
    // o juce.js do JUCE é um módulo (export); servimos como script comum que expõe window.Juce
    if (nome == "juce.js")
    {
        auto codigo = String::fromUTF8 (BinaryData::index_js, BinaryData::index_jsSize)
                          .replace ("import \"./check_native_interop.js\";", "")
                          .replace ("export {", "window.Juce = {");
        codigo = "if (window.__JUCE__) {\n" + codigo + "\n}\n";
        const auto utf8 = codigo.toUTF8();
        const auto* d = reinterpret_cast<const std::byte*> (utf8.getAddress());
        return WebBrowserComponent::Resource { std::vector<std::byte> (d, d + utf8.sizeInBytes() - 1), String ("text/javascript") };
    }
    for (const auto& a : arquivos)
        if (nome == a.url)
            return WebBrowserComponent::Resource { std::vector<std::byte> ((const std::byte*) a.dados, (const std::byte*) a.dados + a.tamanho),
                                                   String (a.mime) };
    return std::nullopt;
}

void DecantaEditor::timerCallback()
{
    // medidores e estado para a página
    DynamicObject::Ptr d (new DynamicObject());
    d->setProperty ("entrada", processador.medEntrada.load());
    d->setProperty ("saida", processador.medSaida.load());
    d->setProperty ("reducao", processador.medReducao.load());
    d->setProperty ("tratando", processador.tratando.load());
    const double sr = processador.taxa.load();
    d->setProperty ("latenciaMs", sr > 0.0 ? 1000.0 * processador.latenciaAmostras.load() / sr : 0.0);
    d->setProperty ("taxa", sr);
    pagina->emitEventIfBrowserIsVisible ("medidores", var (d.get()));
}

void DecantaEditor::paint (Graphics& g)
{
    g.fillAll (Colour (0xff1d1c1b));
}

void DecantaEditor::resized()
{
    pagina->setBounds (getLocalBounds());
}

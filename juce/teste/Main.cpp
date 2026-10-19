// Testa o plugin compilado como um programa faria: carrega o VST3 (ou AU), passa um WAV em blocos
// e grava a saída em WAV float 32.
//
//   testa-plugin <plugin.vst3|.component> <entrada.wav> <saida.wav> [bloco] [param=valor ...]
//   testa-plugin --janela <plugin.vst3|.component> [segundos]
//       abre a janela do plugin por alguns segundos (com DECANTA_TESTE_JANELA=<arquivo>,
//       a página grava ali o que desenhou)
//
// Os valores dos parâmetros são os reais (ex.: separacao=100 ambiencia=-18 soFundo=1).

#include <juce_audio_formats/juce_audio_formats.h>
#include <juce_audio_processors/juce_audio_processors.h>

#include <iostream>

using namespace juce;

static int falha (const String& msg)
{
    std::cerr << "ERRO: " << msg << std::endl;
    return 1;
}

static int abrirJanela (AudioPluginFormatManager& formatos, const File& arquivoPlugin, double segundos)
{
    OwnedArray<PluginDescription> descricoes;
    for (auto* f : formatos.getFormats())
        if (f->fileMightContainThisPluginType (arquivoPlugin.getFullPathName()))
            f->findAllTypesForFile (descricoes, arquivoPlugin.getFullPathName());
    if (descricoes.isEmpty())
        return falha ("plugin não encontrado");
    String erro;
    auto plugin = formatos.createPluginInstance (*descricoes[0], 48000.0, 512, erro);
    if (plugin == nullptr)
        return falha ("não consegui abrir o plugin: " + erro);
    plugin->prepareToPlay (48000.0, 512);
    std::unique_ptr<AudioProcessorEditor> editor (plugin->createEditorIfNeeded());
    if (editor == nullptr)
        return falha ("o plugin não criou a janela");
    std::cout << "janela " << editor->getWidth() << "x" << editor->getHeight() << std::endl;
    DocumentWindow janela ("Decanta (teste)", Colours::black, 0);
    janela.setContentNonOwned (editor.get(), true);
    janela.setTopLeftPosition (80, 80);
    janela.setVisible (true);
    MessageManager::getInstance()->runDispatchLoopUntil ((int) (segundos * 1000));
    janela.clearContentComponent();
    editor.reset();
    plugin->releaseResources();
    return 0;
}

int main (int argc, char** argv)
{
    ScopedJuceInitialiser_GUI juce;
    if (argc >= 3 && String (argv[1]) == "--janela")
    {
        AudioPluginFormatManager formatos;
        formatos.addFormat (new VST3PluginFormat());
       #if JUCE_PLUGINHOST_AU
        formatos.addFormat (new AudioUnitPluginFormat());
       #endif
        return abrirJanela (formatos, File { String (argv[2]) }, argc > 3 ? String (argv[3]).getDoubleValue() : 5.0);
    }
    if (argc < 4)
        return falha ("uso: testa-plugin <plugin> <entrada.wav> <saida.wav> [bloco] [param=valor ...]");

    const File arquivoPlugin { String (argv[1]) }, entrada { String (argv[2]) }, saida { String (argv[3]) };
    const int bloco = argc > 4 ? String (argv[4]).getIntValue() : 512;

    AudioPluginFormatManager formatos;
    formatos.addFormat (new VST3PluginFormat());
   #if JUCE_PLUGINHOST_AU
    formatos.addFormat (new AudioUnitPluginFormat());
   #endif

    OwnedArray<PluginDescription> descricoes;
    for (auto* f : formatos.getFormats())
        if (f->fileMightContainThisPluginType (arquivoPlugin.getFullPathName()))
            f->findAllTypesForFile (descricoes, arquivoPlugin.getFullPathName());
    if (descricoes.isEmpty())
        return falha ("plugin não encontrado: " + arquivoPlugin.getFullPathName());

    AudioFormatManager leitores;
    leitores.registerBasicFormats();
    std::unique_ptr<AudioFormatReader> leitor (leitores.createReaderFor (entrada));
    if (leitor == nullptr)
        return falha ("não consegui ler " + entrada.getFullPathName());
    const int nCanais = (int) leitor->numChannels;
    const int nAmostras = (int) leitor->lengthInSamples;
    const double sr = leitor->sampleRate;
    AudioBuffer<float> audio (nCanais, nAmostras);
    leitor->read (&audio, 0, nAmostras, 0, true, true);

    String erro;
    auto plugin = formatos.createPluginInstance (*descricoes[0], sr, bloco, erro);
    if (plugin == nullptr)
        return falha ("não consegui abrir o plugin: " + erro);

    const auto conjunto = nCanais == 1 ? AudioChannelSet::mono() : AudioChannelSet::stereo();
    AudioProcessor::BusesLayout layout;
    layout.inputBuses.add (conjunto);
    layout.outputBuses.add (conjunto);
    if (!plugin->setBusesLayout (layout))
        return falha ("o plugin não aceitou " + String (nCanais) + " canais");

    // parâmetros pedidos na linha de comando (valor real → normalizado)
    for (int i = 5; i < argc; ++i)
    {
        const String par (argv[i]);
        const String nome = par.upToFirstOccurrenceOf ("=", false, false);
        const float valor = par.fromFirstOccurrenceOf ("=", false, false).getFloatValue();
        bool achou = false;
        for (auto* p : plugin->getParameters())
            if (auto* ph = dynamic_cast<HostedAudioProcessorParameter*> (p))
                if (ph->getParameterID() == nome)
                {
                    auto* r = dynamic_cast<RangedAudioParameter*> (p);
                    p->setValue (r != nullptr ? r->convertTo0to1 (valor) : valor);
                    achou = true;
                }
        if (!achou)
        {
            // pelo nome visível (o VST3 nem sempre expõe o ID)
            for (auto* p : plugin->getParameters())
                if (p->getName (64).toLowerCase().removeCharacters (" -") == nome.toLowerCase())
                {
                    const auto texto = String (valor);
                    p->setValue (p->getValueForText (texto));
                    achou = true;
                }
        }
        if (!achou)
            return falha ("parâmetro desconhecido: " + nome);
    }

    plugin->prepareToPlay (sr, bloco);
    std::cout << "latencia " << plugin->getLatencySamples() << std::endl;

    MidiBuffer midi;
    for (int pos = 0; pos < nAmostras; pos += bloco)
    {
        const int n = jmin (bloco, nAmostras - pos);
        AudioBuffer<float> trecho (audio.getArrayOfWritePointers(), nCanais, pos, n);
        plugin->processBlock (trecho, midi);
    }
    plugin->releaseResources();

    saida.deleteFile();
    WavAudioFormat wav;
    std::unique_ptr<AudioFormatWriter> escritor (wav.createWriterFor (new FileOutputStream (saida), sr, (unsigned) nCanais, 32, {}, 0));
    if (escritor == nullptr)
        return falha ("não consegui gravar " + saida.getFullPathName());
    escritor->writeFromAudioSampleBuffer (audio, 0, nAmostras);
    std::cout << "ok " << nAmostras << " amostras, " << nCanais << " canais" << std::endl;
    return 0;
}

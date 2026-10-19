#include "PluginProcessor.h"
#include "PluginEditor.h"

using namespace juce;

static String formatarDb (float v, int)
{
    if (v <= -60.0f)
        return "-inf dB";
    return String (v, 1) + " dB";
}

AudioProcessorValueTreeState::ParameterLayout DecantaProcessor::criarParametros()
{
    using P = AudioParameterFloat;
    using A = AudioParameterFloatAttributes;
    std::vector<std::unique_ptr<RangedAudioParameter>> p;

    // principais (padrão = o som do app: separação 20 dB, ambiência desligada, sem compressor)
    p.push_back (std::make_unique<P> (ParameterID { ID::voz, 1 }, "Voz", NormalisableRange<float> (-60.0f, 6.0f, 0.1f), 0.0f,
                                      A().withLabel ("dB").withStringFromValueFunction (formatarDb)));
    p.push_back (std::make_unique<P> (ParameterID { ID::separacao, 1 }, "Separacao", NormalisableRange<float> (0.0f, 100.0f, 1.0f), 20.0f,
                                      A().withLabel ("dB")));
    p.push_back (std::make_unique<P> (ParameterID { ID::ambiencia, 1 }, "Ambiencia", NormalisableRange<float> (-60.0f, 6.0f, 0.1f), -60.0f,
                                      A().withLabel ("dB").withStringFromValueFunction (formatarDb)));
    p.push_back (std::make_unique<P> (ParameterID { ID::compressao, 1 }, "Compressao", NormalisableRange<float> (0.0f, 100.0f, 1.0f), 0.0f,
                                      A().withLabel ("%")));
    p.push_back (std::make_unique<P> (ParameterID { ID::ganho, 1 }, "Ganho", NormalisableRange<float> (0.0f, 12.0f, 0.1f), 0.0f,
                                      A().withLabel ("dB")));
    p.push_back (std::make_unique<AudioParameterBool> (ParameterID { ID::soFundo, 1 }, "So fundo", false));

    // compressor completo (ajustes avançados)
    p.push_back (std::make_unique<AudioParameterBool> (ParameterID { ID::compManual, 1 }, "Comp manual", false));
    p.push_back (std::make_unique<P> (ParameterID { ID::threshold, 1 }, "Threshold", NormalisableRange<float> (-60.0f, 0.0f, 0.1f), -22.0f,
                                      A().withLabel ("dB")));
    p.push_back (std::make_unique<P> (ParameterID { ID::ratio, 1 }, "Ratio", NormalisableRange<float> (1.0f, 20.0f, 0.1f, 0.5f), 3.0f,
                                      A().withLabel (":1")));
    p.push_back (std::make_unique<P> (ParameterID { ID::attack, 1 }, "Attack", NormalisableRange<float> (0.1f, 100.0f, 0.1f, 0.4f), 8.0f,
                                      A().withLabel ("ms")));
    p.push_back (std::make_unique<P> (ParameterID { ID::release, 1 }, "Release", NormalisableRange<float> (10.0f, 1000.0f, 1.0f, 0.4f), 150.0f,
                                      A().withLabel ("ms")));
    p.push_back (std::make_unique<P> (ParameterID { ID::knee, 1 }, "Knee", NormalisableRange<float> (0.0f, 12.0f, 0.1f), 6.0f,
                                      A().withLabel ("dB")));

    // ajuste fino da separação (os mesmos do app)
    p.push_back (std::make_unique<P> (ParameterID { ID::posFiltro, 1 }, "Pos-filtro", NormalisableRange<float> (0.0f, 0.2f, 0.01f), 0.0f));
    p.push_back (std::make_unique<P> (ParameterID { ID::limiarRuido, 1 }, "Limiar ruido", NormalisableRange<float> (-40.0f, 10.0f, 1.0f), -15.0f,
                                      A().withLabel ("dB")));

    return { p.begin(), p.end() };
}

DecantaProcessor::DecantaProcessor()
    : AudioProcessor (BusesProperties()
                          .withInput ("Entrada", AudioChannelSet::stereo(), true)
                          .withOutput ("Saida", AudioChannelSet::stereo(), true)),
      state (*this, nullptr, "DECANTA", criarParametros())
{
    pVoz = state.getRawParameterValue (ID::voz);
    pSeparacao = state.getRawParameterValue (ID::separacao);
    pAmbiencia = state.getRawParameterValue (ID::ambiencia);
    pCompressao = state.getRawParameterValue (ID::compressao);
    pGanho = state.getRawParameterValue (ID::ganho);
    pSoFundo = state.getRawParameterValue (ID::soFundo);
    pCompManual = state.getRawParameterValue (ID::compManual);
    pThreshold = state.getRawParameterValue (ID::threshold);
    pRatio = state.getRawParameterValue (ID::ratio);
    pAttack = state.getRawParameterValue (ID::attack);
    pRelease = state.getRawParameterValue (ID::release);
    pKnee = state.getRawParameterValue (ID::knee);
    pPosFiltro = state.getRawParameterValue (ID::posFiltro);
    pLimiarRuido = state.getRawParameterValue (ID::limiarRuido);
}

DecantaProcessor::~DecantaProcessor()
{
    releaseResources();
}

bool DecantaProcessor::isBusesLayoutSupported (const BusesLayout& layouts) const
{
    const auto entrada = layouts.getMainInputChannelSet();
    const auto saida = layouts.getMainOutputChannelSet();
    if (entrada != saida)
        return false;
    return saida == AudioChannelSet::mono() || saida == AudioChannelSet::stereo();
}

void DecantaProcessor::prepareToPlay (double sampleRate, int)
{
    releaseResources();
    // carrega o modelo aqui (fora da thread de áudio)
    motor = decanta_rt_novo ((uint32_t) jmax (1, getTotalNumOutputChannels()), (uint32_t) std::lround (sampleRate));
    const int latencia = motor != nullptr ? (int) decanta_rt_latencia (motor) : 0;
    setLatencySamples (latencia);
    latenciaAmostras = latencia;
    taxa = sampleRate;
    tratando = motor != nullptr && decanta_rt_ativo (motor) != 0;
}

void DecantaProcessor::releaseResources()
{
    if (motor != nullptr)
    {
        decanta_rt_liberar (motor);
        motor = nullptr;
    }
}

double DecantaProcessor::getTailLengthSeconds() const
{
    const double sr = taxa.load();
    return sr > 0.0 ? latenciaAmostras.load() / sr : 0.0;
}

DecantaAjustesRT DecantaProcessor::lerAjustes() const
{
    DecantaAjustesRT a;
    a.separacao_db = pSeparacao->load();
    a.pos_filtro = pPosFiltro->load();
    a.limiar_ruido_db = pLimiarRuido->load();
    a.voz_db = pVoz->load();
    a.ambiencia_db = pAmbiencia->load();
    a.ganho_db = pGanho->load();
    a.compressao = pCompressao->load() / 100.0f;
    a.comp_manual = pCompManual->load() > 0.5f ? 1 : 0;
    a.threshold_db = pThreshold->load();
    a.ratio = pRatio->load();
    a.attack_ms = pAttack->load();
    a.release_ms = pRelease->load();
    a.knee_db = pKnee->load();
    a.so_fundo = pSoFundo->load() > 0.5f ? 1 : 0;
    return a;
}

static void guardarPico (std::atomic<float>& m, float novo)
{
    // queda lenta: o timer da janela lê ~20 vezes por segundo
    const float atual = m.load();
    m.store (jmax (novo, atual * 0.85f));
}

void DecantaProcessor::processBlock (AudioBuffer<float>& buffer, MidiBuffer&)
{
    ScopedNoDenormals semDenormais;
    const int nCanais = jmin (getTotalNumInputChannels(), getTotalNumOutputChannels(), buffer.getNumChannels());
    const int n = buffer.getNumSamples();
    if (motor == nullptr || nCanais <= 0 || n <= 0)
        return;

    const DecantaAjustesRT aj = lerAjustes();
    DecantaMedidores med {};
    float* const* canais = buffer.getArrayOfWritePointers();
    const int r = decanta_rt_processar (motor, canais, canais, (uint32_t) nCanais, (uint32_t) n, &aj, &med);
    if (r != 0)
        return; // em caso de erro o áudio passa como veio

    guardarPico (medEntrada, med.entrada);
    guardarPico (medSaida, med.saida);
    medReducao.store (jmin (med.reducao_db, medReducao.load() * 0.85f));
}

void DecantaProcessor::getStateInformation (MemoryBlock& destData)
{
    if (auto xml = state.copyState().createXml())
        copyXmlToBinary (*xml, destData);
}

void DecantaProcessor::setStateInformation (const void* data, int sizeInBytes)
{
    if (auto xml = getXmlFromBinary (data, sizeInBytes))
        if (xml->hasTagName (state.state.getType()))
            state.replaceState (ValueTree::fromXml (*xml));
}

AudioProcessorEditor* DecantaProcessor::createEditor()
{
    return new DecantaEditor (*this);
}

AudioProcessor* JUCE_CALLTYPE createPluginFilter()
{
    return new DecantaProcessor();
}

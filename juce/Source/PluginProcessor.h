// Decanta (VST3/AU): liga os parâmetros do programa ao motor em tempo real.

#pragma once

#include <juce_audio_processors/juce_audio_processors.h>

#include "Ponte.h"

#include <atomic>

namespace ID
{
    // nomes dos parâmetros (também usados pela janela)
    inline constexpr const char* voz         = "voz";
    inline constexpr const char* separacao   = "separacao";
    inline constexpr const char* ambiencia   = "ambiencia";
    inline constexpr const char* compressao  = "compressao";
    inline constexpr const char* ganho       = "ganho";
    inline constexpr const char* soFundo     = "soFundo";
    inline constexpr const char* compManual  = "compManual";
    inline constexpr const char* threshold   = "threshold";
    inline constexpr const char* ratio       = "ratio";
    inline constexpr const char* attack      = "attack";
    inline constexpr const char* release     = "release";
    inline constexpr const char* knee        = "knee";
    inline constexpr const char* posFiltro   = "posFiltro";
    inline constexpr const char* limiarRuido = "limiarRuido";
}

class DecantaProcessor : public juce::AudioProcessor
{
public:
    DecantaProcessor();
    ~DecantaProcessor() override;

    void prepareToPlay (double sampleRate, int samplesPerBlock) override;
    void releaseResources() override;
    bool isBusesLayoutSupported (const BusesLayout& layouts) const override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    using AudioProcessor::processBlock;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }

    const juce::String getName() const override { return JucePlugin_Name; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override;

    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

    void getStateInformation (juce::MemoryBlock& destData) override;
    void setStateInformation (const void* data, int sizeInBytes) override;

    juce::AudioProcessorValueTreeState state;

    // medidores para a janela (pico com queda lenta, lidos pelo timer da janela)
    std::atomic<float> medEntrada { 0.0f }, medSaida { 0.0f }, medReducao { 0.0f };
    std::atomic<bool> tratando { false };
    std::atomic<int> latenciaAmostras { 0 };
    std::atomic<double> taxa { 0.0 };

private:
    static juce::AudioProcessorValueTreeState::ParameterLayout criarParametros();
    DecantaAjustesRT lerAjustes() const;

    DecantaRT* motor = nullptr;

    std::atomic<float>* pVoz; std::atomic<float>* pSeparacao; std::atomic<float>* pAmbiencia;
    std::atomic<float>* pCompressao; std::atomic<float>* pGanho; std::atomic<float>* pSoFundo;
    std::atomic<float>* pCompManual; std::atomic<float>* pThreshold; std::atomic<float>* pRatio;
    std::atomic<float>* pAttack; std::atomic<float>* pRelease; std::atomic<float>* pKnee;
    std::atomic<float>* pPosFiltro; std::atomic<float>* pLimiarRuido;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (DecantaProcessor)
};

// Decanta: janela do plugin (página HTML em ui/, ligada aos parâmetros).

#pragma once

#include <juce_gui_extra/juce_gui_extra.h>

#include "PluginProcessor.h"

#include <memory>
#include <vector>

class DecantaEditor : public juce::AudioProcessorEditor, private juce::Timer
{
public:
    explicit DecantaEditor (DecantaProcessor&);
    ~DecantaEditor() override;

    void resized() override;
    void paint (juce::Graphics&) override;

private:
    void timerCallback() override;
    std::optional<juce::WebBrowserComponent::Resource> recurso (const juce::String& url);
    juce::WebBrowserComponent::Options opcoes();

    DecantaProcessor& processador;

    // um "relay" por parâmetro (nome = ID do parâmetro, usado pelo app.js)
    std::vector<std::unique_ptr<juce::WebSliderRelay>> sliders;
    std::vector<std::unique_ptr<juce::WebToggleButtonRelay>> chaves;

    std::unique_ptr<juce::WebBrowserComponent> pagina;

    std::vector<std::unique_ptr<juce::WebSliderParameterAttachment>> ligacoesSliders;
    std::vector<std::unique_ptr<juce::WebToggleButtonParameterAttachment>> ligacoesChaves;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (DecantaEditor)
};

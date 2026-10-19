// Decanta: janela do plugin. Cada botão é ligado a um parâmetro pelo JUCE (juce.js).
// Aberta num navegador comum (sem o plugin), usa valores de demonstração.
// Script comum (não módulo): o WebKit do macOS não carrega módulos pelo endereço do plugin.

// faixa e padrão de cada parâmetro (os mesmos de PluginProcessor.cpp)
const PARAMS = {
  voz:         { start: -60, end: 6,    skew: 1,   padrao: 0 },
  separacao:   { start: 0,   end: 100,  skew: 1,   padrao: 20 },
  ambiencia:   { start: -60, end: 6,    skew: 1,   padrao: -60 },
  compressao:  { start: 0,   end: 100,  skew: 1,   padrao: 0 },
  ganho:       { start: 0,   end: 12,   skew: 1,   padrao: 0 },
  threshold:   { start: -60, end: 0,    skew: 1,   padrao: -22 },
  ratio:       { start: 1,   end: 20,   skew: 0.5, padrao: 3 },
  attack:      { start: 0.1, end: 100,  skew: 0.4, padrao: 8 },
  release:     { start: 10,  end: 1000, skew: 0.4, padrao: 150 },
  knee:        { start: 0,   end: 12,   skew: 1,   padrao: 6 },
  posFiltro:   { start: 0,   end: 0.2,  skew: 1,   padrao: 0 },
  limiarRuido: { start: -40, end: 10,   skew: 1,   padrao: -15 },
};
const COMP_AVANCADO = ["threshold", "ratio", "attack", "release", "knee"];

const num = (v, casas = 0) => v.toFixed(casas).replace("-", "−").replace(".", ",");
const FORMATO = {
  voz: (v) => (v <= -60 ? "−∞" : num(v, 1) + " dB"),
  ambiencia: (v) => (v <= -60 ? "−∞" : num(v, 1) + " dB"),
  separacao: (v) => num(v) + " dB",
  compressao: (v) => num(v) + "%",
  ganho: (v) => "+" + num(v, 1) + " dB",
  threshold: (v) => num(v, 1) + " dB",
  ratio: (v) => num(v, 1) + " : 1",
  attack: (v) => num(v, v < 10 ? 1 : 0) + " ms",
  release: (v) => num(v) + " ms",
  knee: (v) => num(v, 1) + " dB",
  posFiltro: (v) => (v <= 0 ? "desligado" : num(v, 2)),
  limiarRuido: (v) => num(v) + " dB",
};

// ---------------------------------------------------------------------------------------------
// estado dos parâmetros: o do JUCE, ou um de demonstração

function ouvintes() {
  const l = new Set();
  return { addListener: (f) => l.add(f), callListeners: () => l.forEach((f) => f()) };
}

function sliderDemo(nome, valor) {
  const p = PARAMS[nome];
  const s = {
    properties: { start: p.start, end: p.end, skew: p.skew, interval: 0 },
    valor,
    valueChangedEvent: ouvintes(),
    getScaledValue: () => s.valor,
    getNormalisedValue: () => Math.pow((s.valor - p.start) / (p.end - p.start), p.skew),
    setNormalisedValue: (n) => {
      s.valor = Math.pow(Math.min(1, Math.max(0, n)), 1 / p.skew) * (p.end - p.start) + p.start;
      s.valueChangedEvent.callListeners();
    },
    sliderDragStarted: () => {},
    sliderDragEnded: () => {},
  };
  return s;
}

function chaveDemo(valor) {
  const c = { valor, valueChangedEvent: ouvintes(), getValue: () => c.valor, setValue: (v) => { c.valor = v; c.valueChangedEvent.callListeners(); } };
  return c;
}

const DEMO = { voz: 0, separacao: 20, ambiencia: -18, compressao: 45, ganho: 6 };

// juce.js (servido pelo plugin) expõe window.Juce; fora do plugin não existe
const juce = window.__JUCE__ && window.Juce ? window.Juce : null;

const sliders = {};
for (const nome of Object.keys(PARAMS))
  sliders[nome] = juce ? juce.getSliderState(nome) : sliderDemo(nome, DEMO[nome] ?? PARAMS[nome].padrao);
const chaves = {
  soFundo: juce ? juce.getToggleState("soFundo") : chaveDemo(false),
  compManual: juce ? juce.getToggleState("compManual") : chaveDemo(false),
};

function normalizado(nome, valor) {
  const p = PARAMS[nome];
  return Math.pow((valor - p.start) / (p.end - p.start), p.skew);
}

function definir(nome, valor) {
  const s = sliders[nome];
  s.sliderDragStarted();
  s.setNormalisedValue(normalizado(nome, valor));
  s.sliderDragEnded();
}

// ---------------------------------------------------------------------------------------------
// botões giratórios: 270° de curso, arrastar para cima/baixo, duplo clique ou ⌥+clique = padrão

function ponto(r, graus) {
  const a = ((graus - 90) * Math.PI) / 180;
  return [50 + r * Math.cos(a), 50 + r * Math.sin(a)];
}
function arco(r, de, ate) {
  const [x1, y1] = ponto(r, de), [x2, y2] = ponto(r, ate);
  return `M${x1.toFixed(2)} ${y1.toFixed(2)} A${r} ${r} 0 ${ate - de > 180 ? 1 : 0} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}

function desenharKnob(el) {
  const nome = el.dataset.param, s = sliders[nome];
  const v = Math.min(1, Math.max(0, s.getNormalisedValue()));
  const ang = -135 + 270 * v;
  const [px, py] = ponto(19, ang);
  el.querySelector("svg").innerHTML =
    `<path d="${arco(40, -135, 135)}" fill="none" stroke="#3a3835" stroke-width="6" stroke-linecap="round"/>` +
    (v > 0.001 ? `<path d="${arco(40, -135, ang)}" fill="none" stroke="${el.dataset.cor}" stroke-width="6" stroke-linecap="round"/>` : "") +
    `<circle cx="50" cy="50" r="28" fill="#34322f" stroke="#4a4742" stroke-width="1.5"/>` +
    `<line x1="50" y1="50" x2="${px.toFixed(2)}" y2="${py.toFixed(2)}" stroke="#efe5cf" stroke-width="4" stroke-linecap="round"/>`;
  el.querySelector(".v").textContent = FORMATO[nome](s.getScaledValue());
}

for (const el of document.querySelectorAll(".knob")) {
  const nome = el.dataset.param, s = sliders[nome];
  const svg = el.querySelector("svg");
  const tam = el.dataset.tam;
  svg.setAttribute("viewBox", "0 0 100 100");
  svg.setAttribute("width", tam);
  svg.setAttribute("height", tam);
  s.valueChangedEvent.addListener(() => desenharKnob(el));
  desenharKnob(el);

  svg.addEventListener("pointerdown", (e) => {
    if (e.altKey) {
      definir(nome, PARAMS[nome].padrao);
      return;
    }
    svg.setPointerCapture(e.pointerId);
    const y0 = e.clientY, n0 = s.getNormalisedValue();
    s.sliderDragStarted();
    const mover = (ev) => {
      const fino = ev.shiftKey ? 0.2 : 1;
      s.setNormalisedValue(n0 + ((y0 - ev.clientY) / 200) * fino);
    };
    const soltar = () => {
      svg.removeEventListener("pointermove", mover);
      svg.removeEventListener("pointerup", soltar);
      svg.removeEventListener("pointercancel", soltar);
      s.sliderDragEnded();
    };
    svg.addEventListener("pointermove", mover);
    svg.addEventListener("pointerup", soltar);
    svg.addEventListener("pointercancel", soltar);
  });
  svg.addEventListener("dblclick", () => definir(nome, PARAMS[nome].padrao));
  svg.addEventListener("wheel", (e) => {
    e.preventDefault();
    s.sliderDragStarted();
    s.setNormalisedValue(s.getNormalisedValue() - e.deltaY / 1500);
    s.sliderDragEnded();
  }, { passive: false });
}

// ---------------------------------------------------------------------------------------------
// controles deslizantes dos ajustes avançados

function desenharLinha(el) {
  const nome = el.dataset.param, s = sliders[nome];
  const v = Math.min(1, Math.max(0, s.getNormalisedValue()));
  el.querySelector(".trilho").style.setProperty("--p", (v * 100).toFixed(2) + "%");
  el.querySelector("b").textContent = FORMATO[nome](s.getScaledValue());
}

for (const el of document.querySelectorAll(".linha")) {
  const nome = el.dataset.param, s = sliders[nome];
  const trilho = el.querySelector(".trilho");
  s.valueChangedEvent.addListener(() => desenharLinha(el));
  desenharLinha(el);
  const deX = (e) => {
    const r = trilho.getBoundingClientRect();
    return (e.clientX - r.left) / r.width;
  };
  trilho.addEventListener("pointerdown", (e) => {
    if (COMP_AVANCADO.includes(nome) && !chaves.compManual.getValue()) chaves.compManual.setValue(true);
    if (e.altKey) {
      definir(nome, PARAMS[nome].padrao);
      return;
    }
    trilho.setPointerCapture(e.pointerId);
    s.sliderDragStarted();
    s.setNormalisedValue(deX(e));
    const mover = (ev) => s.setNormalisedValue(deX(ev));
    const soltar = () => {
      trilho.removeEventListener("pointermove", mover);
      trilho.removeEventListener("pointerup", soltar);
      s.sliderDragEnded();
    };
    trilho.addEventListener("pointermove", mover);
    trilho.addEventListener("pointerup", soltar);
  });
}

// ---------------------------------------------------------------------------------------------
// chaves e painel avançado

const btnSoFundo = document.getElementById("soFundo");
const atualizarSoFundo = () => btnSoFundo.classList.toggle("ligada", !!chaves.soFundo.getValue());
chaves.soFundo.valueChangedEvent.addListener(atualizarSoFundo);
btnSoFundo.addEventListener("click", () => chaves.soFundo.setValue(!chaves.soFundo.getValue()));
atualizarSoFundo();

const knobComp = document.querySelector('.knob[data-param="compressao"]');
const marcaManual = document.createElement("span");
marcaManual.className = "manual";
knobComp.appendChild(marcaManual);
const modoComp = document.getElementById("modoComp");
const atualizarModo = () => {
  const manual = !!chaves.compManual.getValue();
  marcaManual.textContent = manual ? "MANUAL ⚙" : "";
  modoComp.textContent = manual ? "MANUAL" : "AUTOMÁTICO";
  modoComp.classList.toggle("manual", manual);
};
chaves.compManual.valueChangedEvent.addListener(atualizarModo);
atualizarModo();
// mexer no botão Compressão volta ao modo automático
sliders.compressao.valueChangedEvent.addListener(() => {
  if (arrastandoCompressao && chaves.compManual.getValue()) chaves.compManual.setValue(false);
});
let arrastandoCompressao = false;
knobComp.querySelector("svg").addEventListener("pointerdown", () => (arrastandoCompressao = true));
window.addEventListener("pointerup", () => (arrastandoCompressao = false));

const painel = document.getElementById("avancado");
const engrenagem = document.getElementById("engrenagem");
const abrirFechar = (abrir) => {
  painel.classList.toggle("oculto", !abrir);
  engrenagem.classList.toggle("ativa", abrir);
  engrenagem.textContent = abrir ? "✕" : "⚙";
};
engrenagem.addEventListener("click", () => abrirFechar(painel.classList.contains("oculto")));
document.getElementById("fechar").addEventListener("click", () => abrirFechar(false));
document.getElementById("restaurar").addEventListener("click", () => {
  for (const nome of [...COMP_AVANCADO, "posFiltro", "limiarRuido"]) definir(nome, PARAMS[nome].padrao);
  chaves.compManual.setValue(false);
});

// ---------------------------------------------------------------------------------------------
// medidores e selo (eventos do plugin, ~20 por segundo)

const altura = (pico) => {
  const db = pico > 1e-6 ? 20 * Math.log10(pico) : -100;
  return Math.max(0, Math.min(1, (db + 60) / 60)) * 100 + "%";
};
const medE = document.getElementById("medEntrada"), medS = document.getElementById("medSaida");
const gr = document.getElementById("gr"), selo = document.getElementById("selo"), rodape = document.getElementById("rodape");

function mostrar(d) {
  medE.style.setProperty("--n", altura(d.entrada));
  medS.style.setProperty("--n", altura(d.saida));
  gr.style.width = Math.min(1, -d.reducao / 20) * 100 + "%";
  if (d.tratando) {
    selo.className = "selo rt";
    const khz = d.taxa > 0 && Math.round(d.taxa) !== 48000 ? ` · ${num(d.taxa / 1000, 1)} KHZ` : "";
    selo.textContent = `TEMPO REAL · ${Math.round(d.latenciaMs)} MS${khz}`;
    rodape.textContent = "Leva um instante para “pegar” após o play";
  } else {
    selo.className = "selo erro";
    selo.textContent = d.taxa > 0 ? `${num(d.taxa / 1000, 1)} KHZ · SEM TRATAMENTO` : "AGUARDANDO ÁUDIO";
    rodape.textContent = d.taxa > 0 ? "Taxa não suportada (use de 8 a 192 kHz)" : "";
  }
}

if (window.__JUCE__) window.__JUCE__.backend.addEventListener("medidores", mostrar);
else mostrar({ entrada: 0.4, saida: 0.5, reducao: -5, tratando: true, latenciaMs: 40, taxa: 48000 });

// conferência para os testes (lida pelo index.html no momento do relatório)
window.__decantaResumo = () => ({
  juce: !!juce,
  arcos: document.querySelectorAll(".knob svg path").length,
  valores: [...document.querySelectorAll(".knob .v")].map((e) => e.textContent),
  selo: selo.textContent,
  fonte: document.fonts.check("12px Archivo"),
});
window.__decantaOk = true;

// Interface em C da ponte com o motor (Rust): ver ponte/src/lib.rs.

#pragma once

#include <cstdint>

extern "C"
{
    struct DecantaAjustesRT
    {
        float separacao_db;
        float pos_filtro;
        float limiar_ruido_db;
        float voz_db;
        float ambiencia_db;
        float ganho_db;
        float compressao; // 0 a 1
        int32_t comp_manual;
        float threshold_db;
        float ratio;
        float attack_ms;
        float release_ms;
        float knee_db;
        int32_t so_fundo;
    };

    struct DecantaMedidores
    {
        float entrada;    // pico, linear
        float saida;      // pico, linear
        float reducao_db; // ≤ 0
    };

    struct DecantaRT;

    DecantaRT* decanta_rt_novo (uint32_t n_canais, uint32_t sr);
    void decanta_rt_liberar (DecantaRT* h);
    uint32_t decanta_rt_latencia (const DecantaRT* h);
    int32_t decanta_rt_ativo (const DecantaRT* h);
    int32_t decanta_rt_reiniciar (DecantaRT* h);
    int32_t decanta_rt_processar (DecantaRT* h, const float* const* entradas, float* const* saidas,
                                  uint32_t n_canais, uint32_t n, const DecantaAjustesRT* aj,
                                  DecantaMedidores* medidores);
}

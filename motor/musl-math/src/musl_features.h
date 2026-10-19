/* Substitui o features.h interno do musl (nome trocado para não colidir com o <features.h> do sistema; só o necessário para src/math). */
#ifndef _MUSL_FEATURES_H
#define _MUSL_FEATURES_H
#if defined(__GNUC__) || defined(__clang__)
#define hidden __attribute__((__visibility__("hidden")))
#else
#define hidden
#endif
#endif

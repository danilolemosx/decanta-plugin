# Decanta Plugin (VST3, AU e AAX) com JUCE 8
#
#   cmake -B build-juce -S juce -DCMAKE_BUILD_TYPE=Release -DJUCE_DIR=/caminho/JUCE
#   cmake --build build-juce --config Release
#
# A ponte em Rust (../ponte) é compilada pelo cargo antes do plugin.
# Mac universal (Apple e Intel): -DDECANTA_UNIVERSAL=ON (precisa dos dois alvos no rustup).

cmake_minimum_required(VERSION 3.22)
option(DECANTA_UNIVERSAL "Mac: compilar para Apple Silicon e Intel" OFF)
set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0" CACHE STRING "")
if(DECANTA_UNIVERSAL)
	set(CMAKE_OSX_ARCHITECTURES "arm64;x86_64" CACHE STRING "" FORCE)
else()
	set(CMAKE_OSX_ARCHITECTURES "arm64" CACHE STRING "")
endif()
project(DecantaPlugin VERSION 0.1.0 LANGUAGES C CXX)

set(JUCE_DIR "$ENV{HOME}/.cache/decanta-aax/JUCE" CACHE PATH "Pasta do JUCE 8")
add_subdirectory("${JUCE_DIR}" JUCE EXCLUDE_FROM_ALL)

# --- ponte (Rust) -------------------------------------------------------------
set(PONTE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../ponte")
# fora da pasta do projeto: caminho com espaços quebra a compilação do mp3lame (motor)
set(PONTE_TARGET_DIR "${CMAKE_BINARY_DIR}/ponte-target" CACHE PATH "Pasta de compilação da ponte")
set(CARGO_BUILD cargo build --release --lib --manifest-path "${PONTE_DIR}/Cargo.toml" --target-dir "${PONTE_TARGET_DIR}")

if(WIN32)
	set(PONTE_LIB "${PONTE_TARGET_DIR}/release/decanta_ponte.lib")
	add_custom_target(ponte
		COMMAND ${CARGO_BUILD}
		BYPRODUCTS "${PONTE_LIB}"
		WORKING_DIRECTORY "${PONTE_DIR}"
		COMMENT "Compilando a ponte do motor (Rust)"
		USES_TERMINAL)
elseif(DECANTA_UNIVERSAL)
	set(PONTE_LIB "${PONTE_TARGET_DIR}/universal/libdecanta_ponte.a")
	add_custom_target(ponte
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD} --target aarch64-apple-darwin
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD} --target x86_64-apple-darwin
		COMMAND ${CMAKE_COMMAND} -E make_directory "${PONTE_TARGET_DIR}/universal"
		COMMAND lipo -create
			"${PONTE_TARGET_DIR}/aarch64-apple-darwin/release/libdecanta_ponte.a"
			"${PONTE_TARGET_DIR}/x86_64-apple-darwin/release/libdecanta_ponte.a"
			-output "${PONTE_LIB}"
		BYPRODUCTS "${PONTE_LIB}"
		WORKING_DIRECTORY "${PONTE_DIR}"
		COMMENT "Compilando a ponte do motor (Rust, Apple + Intel)"
		USES_TERMINAL)
else()
	set(PONTE_LIB "${PONTE_TARGET_DIR}/release/libdecanta_ponte.a")
	add_custom_target(ponte
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD}
		BYPRODUCTS "${PONTE_LIB}"
		WORKING_DIRECTORY "${PONTE_DIR}"
		COMMENT "Compilando a ponte do motor (Rust)"
		USES_TERMINAL)
endif()

# --- plugin -------------------------------------------------------------------
if(APPLE)
	set(DECANTA_FORMATOS VST3 AU AAX)
else()
	set(DECANTA_FORMATOS VST3 AAX)
endif()

juce_add_plugin(Decanta
	COMPANY_NAME "Danilo Lemos"
	BUNDLE_ID "br.danilolemos.decanta"
	PLUGIN_MANUFACTURER_CODE DnLm
	PLUGIN_CODE Dcnt
	FORMATS ${DECANTA_FORMATOS}
	AAX_IDENTIFIER br.danilolemos.decanta
	AAX_CATEGORY NoiseReduction
	PRODUCT_NAME "Decanta"
	DESCRIPTION "Limpeza de diálogo com DeepFilterNet"
	IS_SYNTH FALSE
	NEEDS_MIDI_INPUT FALSE
	NEEDS_MIDI_OUTPUT FALSE
	IS_MIDI_EFFECT FALSE
	NEEDS_WEB_BROWSER TRUE
	NEEDS_WEBVIEW2 TRUE
	VST3_CATEGORIES Fx Restoration
	AU_MAIN_TYPE kAudioUnitType_Effect
	COPY_PLUGIN_AFTER_BUILD FALSE)

juce_add_binary_data(DecantaUI SOURCES
	ui/index.html
	ui/style.css
	ui/app.js
	ui/fonts/archivo.woff2
	"${JUCE_DIR}/modules/juce_gui_extra/native/javascript/index.js")

target_sources(Decanta PRIVATE
	Source/PluginProcessor.cpp
	Source/PluginEditor.cpp)

target_compile_definitions(Decanta PUBLIC
	JUCE_WEB_BROWSER=1
	JUCE_USE_CURL=0
	JUCE_VST3_CAN_REPLACE_VST2=0)
if(WIN32)
	# janela em HTML no Windows: WebView2 (Edge), ligado estaticamente
	target_compile_definitions(Decanta PUBLIC JUCE_USE_WIN_WEBVIEW2_WITH_STATIC_LINKING=1)
endif()

add_dependencies(Decanta ponte)
target_link_libraries(Decanta
	PRIVATE
		DecantaUI
		"${PONTE_LIB}"
		juce::juce_audio_utils
		juce::juce_gui_extra
	PUBLIC
		juce::juce_recommended_config_flags
		juce::juce_recommended_lto_flags
		juce::juce_recommended_warning_flags)
if(WIN32)
	# bibliotecas do sistema que a biblioteca padrão do Rust usa
	target_link_libraries(Decanta PRIVATE ws2_32 userenv ntdll bcrypt advapi32 kernel32 synchronization)
endif()

# --- teste: carrega o plugin compilado e processa um WAV ------------------------
juce_add_console_app(testa-plugin PRODUCT_NAME "testa-plugin")
target_sources(testa-plugin PRIVATE teste/Main.cpp)
target_compile_definitions(testa-plugin PRIVATE
	JUCE_PLUGINHOST_VST3=1
	JUCE_PLUGINHOST_AU=$<BOOL:${APPLE}>
	JUCE_MODAL_LOOPS_PERMITTED=1
	JUCE_WEB_BROWSER=0
	JUCE_USE_CURL=0)
target_link_libraries(testa-plugin PRIVATE
	juce::juce_audio_processors
	juce::juce_audio_formats
	juce::juce_recommended_config_flags)
if(APPLE)
	target_link_libraries(testa-plugin PRIVATE "-framework CoreAudioKit")
endif()

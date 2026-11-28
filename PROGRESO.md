# Progreso del revival de Konofan

Fork de [Axel](https://github.com/Assasans/axel) (servidor de fans de *KonoSuba: Fantastic Days*) para jugar en local con servidor propio, controlar banners, probabilidades, eventos y kits, y añadir contenido nuevo. Uso personal.

_Última actualización: 2026-11-12_

## Estado actual

Fase 3–4 (redirigir el cliente y servidor mínimo). El 2026-11-12 quedó montado en el PC de Pablo: WSL2 Ubuntu 24.04 con Axel, PostgreSQL (116.271 probabilidades importadas) y nginx; MuMu con el certificado de sistema y `10.0.2.2 konofan.local` en su hosts (copia del original en `/system/etc/hosts.orig`). Falta la primera partida real.

## Qué funciona (heredado de Axel)

- Tutorial, creación de cuenta y perfil.
- Misiones principales, libres y de fama con recompensas; historias (también colaboraciones).
- Equipo, subir personajes, mejora de objetos, tienda, mazmorras, gacha diario, multijugador básico.
- Cliente stub (`konofd-client-stub` v1.1.0, global 4.11.5) sin LIAPP, con URL del servidor configurable sin recompilar.

## Qué hemos añadido

- **Gacha con probabilidades reales** (rama `gacha-probabilidades-reales`): las tiradas usan `gacha.rates`; la 10.ª de una multi usa la tabla de garantía. Corregido el fallo de `gacha_rate` con `bonus_rates`. Historial de tiradas real (`gacha.user_draws`, endpoint `gachalog`).
- **Arreglos tras la primera partida** (rama `claude/project-thread-hmlqra`, 2026-11-12):
  - Carrusel de la portada: el servidor reactiva los banners de `config/home-banners.toml` (IDs de `master/banner.json`); todos habían caducado al cerrar el juego.
  - Buzón de regalos real: tabla `user_presents`, contador correcto en Gifts, historial (Gift Log), regalo de bienvenida una vez por jugador y `scripts/konofan/enviar-regalo.sh` para mandar regalos. Miembros/armas/accesorios aún no se pueden regalar.
  - Anuncios propios en Note: tabla `news` y páginas de detalle en `/static/news/<id>.html`.
  - Los recursos locales (`resources-root`, `/srv/konofd`) por fin se usan: `serve_dir_with_fallback` servía desde la ruta de la URL y todo acababa yendo al espejo.
- **Segunda tanda de arreglos** (2026-11-12):
  - Jobs (expediciones) reales: tabla `user_expeditions`; cada 864 s un «turno» (máx. 50, 12 h) con 400 eris, 2 de afinidad y una tirada de `expedition_drop`; Accept All, asignar y quitar personaje.
  - Exchange real: cobra las medallas (primer canje con precio `_first`), da el contenido del pack y respeta los límites (NUM/MONTH/WEEK, tabla `user_exchanges`). Las medallas de diosa y los tickets de salto pasan a `user_items` (antes se sumaban en cada login sin guardarse).
  - Rombos rojos: el servidor ya no manda avisos de «nuevo» fijos a 1; el de Jobs se calcula (algún trabajo al máximo).
  - Shop: los productos de pago real (con precio de Google Play) se quedan caducados para que la tienda no espere a Google Play Billing; solo quedan los gratis.
- **Tercera tanda: monedas y consumos** (2026-11-12):
  - Gemas (de pago y gratis), eris y estamina se guardan en `user_wallet` (migración `20261112000004`). Cada cuenta empieza con lo que tenía hasta ahora (85 720 eris, 100 000 + 1 000 000 gemas, 419 de estamina).
  - La estamina se recarga 1 punto cada 3 minutos hasta el máximo del rango (200 en rango 45, maestro `exp`); por encima del máximo no se recarga.
  - Main, Free, Event y Fame gastan la estamina del nivel al empezar (Fame la multiplica por `cost_ratio`). El boss de evento gasta sus tickets (`bossticket` × multiplicador).
  - Cada victoria da las eris y la afinidad del nivel (`money`, `intimacy_exp`) y se guardan; antes la afinidad solo se mostraba.
  - Skip real en Main, Free y Event: gasta tickets de salto y estamina por cada pasada y da las eris y los objetos del nivel. Si falta algo, no gasta ni da nada.
  - `buy`: comprar estamina (200 por 100 gemas, 30 al día), Fame Potions, retos de mazmorra y re-sorteos con gemas o eris; se gastan antes las gemas gratis. Compras guardadas en `user_shop_purchases` para los límites diarios.
  - Regalos, canjes y Jobs suman o cobran las eris y gemas en la base de datos.
  - Las estrellas de las misiones de evento salen doradas (para poder saltarlas).
  - Métodos que el juego llamaba y no existían (cerraban la sesión con «Connection Error»): `itemuse` (usar carne de estamina: 100 o 10 por unidad, maestro `stamina_item`), `fame_supplement_buy` (comprar Fame Potions desde la Fame Quest) y `partycopy` (copiar un equipo en otro).
- **Cuarta tanda: combate** (2026-11-12):
  - Las estadísticas de batalla salían multiplicadas por 20: `MinMaxRange::interpolate` (src/member.rs) tenía activo un multiplicador de depuración heredado de Axel (`POST_MULTIPLIER = 20.0`). Con agilidad y suerte ×20, los enemigos casi nunca actuaban en Auto, fallaban siempre y todo moría de un golpe.
  - Curva de crecimiento real: `min + (max − min) × ((nivel − 1) / 59)^1,2`, que cuadra con los miembros de nivel 3 y 4 de los datos capturados del servidor original (Kazuma nivel 4: 277 de vida).
  - Cada miembro lleva su propia habilidad pasiva (`passiveskill1`) y su resistencia a atributos (`resist_attr`); antes todos llevaban las de Kazuma.
  - PWR de los equipos: cada fila iba con 12 300 fijo (5 × 12 300 = 61 500). Ahora se calcula: base = ⌊0,35·vida + 3·(ataque + ataque mágico) + 1,5·(defensa + def. mágica)⌋; titular = base + 90; suplente = ⌊base × 0,3⌋ (0,4 si es el mismo personaje). Da exactos los valores de los 20 miembros de los datos capturados.
  - Afinidad: el login mandaba cambiados el nivel (`rank`) y los puntos (`rank_progress`); la afinidad sí se guardaba.
- **Quinta tanda: avisos y equipos** (2026-11-12):
  - Friends: el Greeting Log saltaba siempre porque el login mandaba la versión de Axel como saludo (aviso tipo 7, clave 27) y `greeting_list` devolvía un saludo falso de Megumin. Ya no hay saludos (no hay más jugadores).
  - Misiones: el contador rojo venía de avisos fijos (clave 3 = 2, clave 13 = 7) y de un «misión completada» (11210001) en cada combate. Van a 0 hasta que las misiones sean reales.
  - Equipos: Reset vacía el equipo y Suggest lo llena con los personajes más fuertes (uno por personaje: 5 titulares y luego suplentes).
  - `scorechallenge_log` ya no falla cuando el juego no manda `event_id`.
- **Sexta tanda: progreso real** (2026-11-12):
  - Las cuentas nuevas ya no nacen con rango 45, todo completado y todos los personajes. Empiezan con Kazuma, Aqua, Megumin y Darkness; el gacha del tutorial guarda de verdad sus 10 personajes. Monedas iniciales: 10 000 eris y 3000 cuarzos gratis (más los regalos de bienvenida, ahora con 20 tickets de salto). Ya no se regalan todas las armas, 60 000 pociones ni 200 000 medallas.
  - Rango y experiencia del jugador guardados (`user_progress`, maestro `exp`): la estamina máxima sale del rango y al subir de rango se recarga.
  - Progreso de misiones (`user_quest_progress`): la historia se desbloquea con `unlock_clearstage` (partes, capítulos y fases), se guardan las coronas y cada corona da su recompensa la primera vez (`mainmission_reward{1,2,3}_packid`; la 1-1 da a Iris). Hard: 3 intentos al día; Expert: 1. Solo se puede saltar una fase con las 3 coronas, y los saltos gastan intentos y dan experiencia.
  - Los combates dan experiencia a los titulares y se guarda (antes 230 fijo y no se guardaba). Perder ya no da recompensas.
  - Free Quest: solo salen las zonas fijas y las del día de la semana (hora de Japón), con sus intentos diarios. Las zonas de eventos antiguos (las de nombre en japonés) ya no aparecen.
  - Miembros desde regalos, canjes, packs y coronas (`src/user/grant.rs`), con su personaje y habilidades.
  - `scripts/konofan/reiniciar-cuenta.sh`: devuelve una cuenta al estado de después del tutorial.
- **Séptima tanda: Dungeons y Battle Arena** (2026-11-12):
  - Dungeons reales (`user_dungeon_runs`, `user_dungeon_clears`): solo sale la mazmorra de evento de la semana (rota cada jueves a medianoche UTC), con sus 3 zonas desbloqueadas en orden (`unlock_id`). Cada zona tiene 5 pisos con los enemigos y la vida de `battle_wave`/`battle_enemy`; la vida de tu equipo y la de los enemigos se guarda entre pisos y entre intentos. Bendiciones reales (3 a elegir, re-sorteo, curar y revivir). Al ganar el último piso se da el rango (3 = sin bajas, según `dungeon_clear_rank`) y sus recompensas; con la zona superada se puede saltar con tickets. Las mazmorras normales (11–14) no salen porque no tienen imágenes en el cliente.
  - Battle Arena real (`user_score_challenge_best`): 3 intentos al día (hora de Japón) compartidos entre Normal, Hard y Very Hard; la práctica no gasta. Se guarda la mejor puntuación de cada modo; la total suma las tres y sale en el ranking (solo tú). Cada combate da 10 Arena Medals y cada umbral de puntuación total su recompensa una vez (`scorechallenge_reward_achievement`). Sin intentos, el combate cuenta como práctica.
  - El jefe de la Arena no muere a propósito: la puntuación es el daño hecho en el tiempo límite.
- **Octava tanda: combates** (2026-11-12):
  - Ningún combate arrancaba («Data Load Failed», después de cobrar la estamina): el servidor solo mandaba los datos de los titulares y Suggest había puesto suplentes. Ahora se mandan titulares y suplentes en todos los modos (Main, Free, Event, Arena, multijugador).
  - Huecos bloqueados por rango (maestro `content_release`): suplentes desde el rango 5 y filas traseras desde el 8. Los combates no los usan y Suggest no los llena.
  - Suggest aplica los filtros: elementos preferidos (hasta dos, por el atributo de las habilidades), estadística preferida y las casillas de titulares y suplentes.
  - Free Quest: los intentos diarios salían gastados (0/2). El juego muestra «límite − limit», así que hay que mandar los usados.
  - La estamina de rango 1 es 10 en el juego original (maestro `exp`, +5 por rango). Dungeon se desbloquea en rango 35.
  - El combate se congelaba al lanzar una especial: cada fila del equipo guardaba la especial de su hueco (Kazuma, Aqua, Megumin, Darkness) aunque Suggest o Reset cambiaran el titular. Ahora la especial siempre es del titular (`Party::fix_special_skills`), al mostrar el equipo, al sugerir y al empezar el combate.
  - `scripts/konofan/desbloquear-cuenta.sh <jugador> [rango]`: deja una cuenta de pruebas en rango 45 con toda la historia (Normal, Hard, Expert) y Free Quest superadas con 3 coronas, sin dar sus recompensas.
- **Novena tanda: mazmorras y especiales** (2026-11-13):
  - Dungeon se quedaba cargando al volver a entrar con una zona empezada: `dungeon_area_top` mandaba las bendiciones a elegir como `[0,0,0]` y las especiales a 0. Ahora siempre hay 3 bendiciones (también al empezar la zona) y cada miembro lleva su especial.
  - Suggest en Dungeon aplica elementos y estadística, pone un miembro por personaje y el equipo se guarda (`user_dungeon_teams`, migración `20261113000001`) para entrar con él. Reset vuelve a los más fuertes.
  - Especiales propias: las 10 variantes con especial exclusiva (`skill_sp.member_id`, p. ej. Aqua 1014200 → 101021) usan siempre la suya.
  - **Causa real del cuelgue** (encontrada probando respuestas una a una, primero con Pablo y luego jugando en MuMu): en la primera fase de una zona, y sin partida, `dungeon_area_top` no puede llevar `unchoosed_benefit_id_list`. Su mera presencia, incluso con `[0,0,0]`, cuelga el juego o lo devuelve a la portada. Ahora ese campo solo va cuando hay bendiciones que elegir (tras superar una fase), y Back + volver lleva directo a la fase, como en el original.
  - Combates de mazmorra que se ganaban solos: en `dungeon_battle_start`, la vida de los enemigos va en `hp` (no `current_hp`); con otro nombre el juego leía 0. Además se manda el `chest` de la oleada.
  - Rasgo del equipo en mazmorras (`dungeon_stage_party_passive_skill_change`, tabla `user_dungeon_traits`). El diálogo se queda abierto tras cambiarlo (pasa de «Change» a «Remove»); se cierra con Close.
  - Suggest todavía no pone el rasgo de fama (las armas, joyas y ayudante, desde #55).
- **Décima tanda: probando solo en MuMu** (2026-11-13 y 14, Claude juega por adb):
  - Tras un combate salía «Player Rank 1» y se repetían los tutoriales: el resultado borraba y volvía a sumar rango y experiencia. Ahora solo se suma lo ganado.
  - El lanzador quita la barra final de la URL (`…/static`) y daba 404: nginx la acepta con y sin barra.
  - Equipo de la mazmorra: suplentes y cambios a mano (`update_dungeon_team_form`, columna `forms` en `user_dungeon_teams`, migración `20261114000001`). Suggest pone suplentes y respeta las casillas Main/Sub. Los combates llevan los suplentes. Probado con un combate.
  - Misiones (Novice, Regular, Daily): progreso real calculado desde lo guardado (misiones, capítulos, coronas, rango, miembros) y contadores nuevos (días de login, tiradas, trabajos, Arena) en `user_mission_counters`. Recoger (`missionget`) da el pack de la misión y la guarda en `user_missions` (las diarias se renuevan cada día, hora de Japón). Migración `20261114000002`.
  - Pendiente en Misiones: «Accept All» no envía nada y la pestaña Daily sale vacía. Las de evento salen sin progreso.
  - La Tienda se queda en «Now Loading…»: igual que el + de gemas, el juego espera a Google Play Billing (`queryProductDetailsAsync` nunca responde) y no llega a pedir `shopitemlist`. No es del servidor.
- **Undécima tanda: repaso de Story, Team, Smithy y Menu** (2026-11-14 y 15, Claude juega por adb):
  - Misiones: «Clear all Novice» ya no sale completada antes de tiempo, y visitar el personaje de Home cuenta (#31). `missiongetall` recoge todo lo completado respetando las cadenas `enabled_id` (#32), aunque el botón «Accept All» del juego sigue sin enviar la petición.
  - Team, «Use Reserves»: subir habilidades con duplicados los gasta de verdad de la reserva (`member_skill_up`, #33).
  - Herrería (#34, #35, #37): las mejoras cobran materiales y Eris según `equip_*_details`, gastan las armas usadas como material, y ahora el Eris baja en pantalla al momento. La venta (`sale`) funciona con armas y con materiales. La fabricación (`blacksmith`) cobra el coste, pero falta probarla en el juego (no hay materiales).
  - Perfil: rango, miembros y coronas reales, y el título elegido se guarda (#36).
  - Story revisado: todo bien salvo el banner del evento 24041 (404 en el espejo).
- **Duodécima tanda: Trial, Potential Blessing y evento de Año Nuevo** (2026-11-15):
  - Eris: una fila suelta de Eris en `user_items` hacía que el juego mostrara 50.000 más; `Wallet::load` mueve esas filas (Eris, gemas, estamina) al monedero (#39). Las mejoras de la Herrería descuentan el Eris al momento (#37).
  - Trial of the Ancients: costes y estadísticas reales por fase y desbloqueo (`character_enhance_multi_release`, tabla `user_character_enhance`) (#40). Falta probar un desbloqueo en el juego y ver si las estadísticas cuentan en combate.
  - Potential Blessing: tableros reales y desbloqueo con fragmentos (`character_piece_board_release`, tabla `user_character_piece_board`; el cuerpo de la petición aún no se ha visto) (#41).
  - **Ojo:** `character_piece_board_stage_id_list` del login debe ir vacía cuando no hay nada desbloqueado. Con un 0 por tablero se colgaban todos los combates en «Now Loading…» (#45). Mandar en `chest` los lotes propios de cada misión también los colgaba: se deja `10101111,10101120,10101131` (#43, #44).
  - Evento 24011: las fases usan el progreso real y salen bloqueadas hasta superar la anterior; las misiones son las del evento y se pueden recoger; el canje tiene fechas actuales (#42, #43). Las de «superar todas» solo cuentan con todas superadas (#46).
  - Jefe del evento: las victorias cuentan para su lista y sus misiones (#47), y la información del evento da las derrotas reales (#49). El login ya no regala 600 tickets de jefe falsos: se ven los de la base de datos (#48).
  - El cartel de misión recomendada del evento («Defeat the boss 100 times!») sale con el sello Done sin ninguna derrota. El cliente lo calcula solo con el maestro `event_mission_recommend` y sus datos de misiones (ninguna respuesta lleva ese estado); la misión 52401145 sí llega en `mission`, pero en cadena (`enabled_id`). Sin resolver; es solo visual.
- **Decimotercera tanda: Branch, gacha, combate y Suggest** (2026-11-15 a 21):
  - Branch real (`surprise.rs`, tabla `user_surprise_events`, migración `20261115000003`): una jugada al día por evento, combate con tu equipo y premios de verdad en los tres tipos (combate, corto e historia) (#51). Las tarjetas muestran «Occurrences 12/15» y «Next 0m» (parche del maestro `surprise_event_stock_limit`, #52).
  - El gacha cobra: cuarzos gratis primero y luego de pago, la diaria solo de pago y el botón de ticket gasta un ticket (`gachaticket`). Si no llega, responde -110 (#53).
  - Combate: la afinidad y el Trial of the Ancients ya suman en las estadísticas que se mandan al empezar (el juego solo los suma en la ficha). Un equipo con la misma carta en dos huecos ya no cuelga el combate (`Party::drop_repeated_members`) (#54).
  - Suggest con «Equipment» y «Assist»: pone a cada titular la mejor arma de su tipo (o su arma única) según la estadística elegida, la mejor joya (primero las que resisten los elementos elegidos; el orden de los dígitos de resistencia es una suposición) sin repetir ninguna en el equipo, y el ayudante con más estadísticas. El login ya no se rompe si hay un equipo sin datos en el maestro y manda las joyas como joyas (#55).
  - La pantalla de equipo muestra las armas y joyas equipadas (`partyinfo` manda `weapons`/`accessories` con su `item_id_details`) y el selector de armas y joyas (`weaponlist`, `accessorylist`) lista solo las que tienes, con su id único, así que cambiar un arma a mano se guarda bien (#57, #58). En Suggest, «Stats» elige el arma por el ataque dominante de cada personaje: físico o mágico (#58, #59).
  - Jefe del evento: cada victoria da su botín real (medallas de canje 240111/240112, materiales y Eris): la oleada (`battle_wave`) elige normal o raro por peso y se da uno de sus lotes (`wave_id * 100 + n`, raros desde 50), multiplicado por los tickets usados (x1, x2…). Su combate lleva ya la afinidad y el Trial. No hay límite diario: lo limita el gasto de tickets, como en el juego (#56).
  - La afinidad y el Trial también cuentan en Arena, jefe multijugador y mazmorras (en mazmorras todo menos la vida, porque la partida guarda la vida de cada miembro contra el máximo de la pantalla de equipo) (#60).
  - El ayudante ya sale en la preparación de combate nada más entrar al juego: el login manda los ayudantes (antes solo llegaban al abrir Team). Tras una práctica de Arena, el juego ya no muestra 0/3 intentos: `challengenum` es lo que se descuenta (0 en práctica, 1 si no) (#61).
  - «Select Dungeon» salía con un cartel rosa: las mazmorras 2-8 no tienen su imagen (`dungeon_banner_N.png`) en los catálogos del juego. La rotación semanal ya no las elige (#62).
  - Arena: al volver de una práctica, el récord ya no muestra la puntuación de la práctica. El panel se queda con el mayor entre su récord y `modescore`, así que en práctica `modescore` no pasa del récord (#70).
  - La portada de la Arena decía «Calculating Rankings» aunque el ranking funcionaba: el cliente lee `inRankingPeriod` en camelCase (visto en el `dump.cs` sacado con Il2CppDumper) y el servidor lo mandaba como `in_ranking_period` (#72).
  - Armas y joyas desde regalos, canjes y lotes: se guardan en `user_items_equipment`, una fila por copia (`grant_equipment`, nombradas por `item_id_details`). Las joyas se guardan con tipo 6 (migración `20261124000001`). La lista de venta ya no se rompe con joyas (#63).
  - Banners de Reclutar en la base de datos (tabla `gacha.banners`, migración `20261124000002`): sin filas activas salen todos los del maestro (como antes) menos los desactivados; con alguna activa, solo esas, en su `sort_order`, con su tirada diaria (`daily`) y su ventana opcional `start_at`/`end_at`. Se quitó la lista vieja escrita a mano, que no se usaba (#64).
  - Jefe del evento multijugador: cobra sus tickets al empezar (x1, x2…) y al ganar da su botín real, como el individual (oleada del maestro `event_marathon_quest_stage_boss_multi`), además de experiencia y afinidad del equipo. Los compañeros siguen siendo inventados. Antes enseñaba 100 gemas de pago y 5000 gratis que no se daban (#65).
  - «Accept All» de Misiones: el juego sí manda `missiongetall`, con `mission_id_list` (las que ve cobrables). Ahora se cobran solo esas y, en las cadenas (Capítulo 1-2 → 1-3…), todos los pasos de una vez en lugar de uno por pulsación (#65).
  - «Accept All» cobra cadenas de cualquier largo de una vez (las de capítulos llegan a cientos de pasos; antes se cortaba a los 50) y sin tardar: 1375 misiones en menos de 3 s (#66).
  - Branch (Others) colgaba el juego con «Connection Error»: pedía `notificationset`, que no existía, y el servidor caía en pánico con cualquier método desconocido. Ahora `notificationset` responde bien, y un método sin manejador devuelve un éxito vacío y lo apunta en el log (#66).
  - Room Invitation ya no enseña una sala inventada de «Megumin» (entrar habría cobrado tickets de jefe por un combate que no existe) (#66).
  - «Clear all of the Daily Missions» salía cobrable (1/1) con 3 de 7 diarias hechas: el juego la da por hecha con cualquier valor, como la de novato. Ahora vale 1 solo con las 7 cobradas hoy (#67).
  - Branch: las tarjetas decían «Next -17m» (menos los minutos de la hora actual): cada tarjeta llevaba `expired_date: 0`. Ahora llevan la próxima medianoche de Japón, cuando se reinician las jugadas (#68; el intento de #67 con el maestro no era la causa y se deshizo).
  - Perfil: «Party Power» ya no sale N/A: es el poder del equipo más fuerte, sumado como en la pantalla de equipo. «Arena ranking» sale 1.º en cuanto hay una puntuación de Arena (antes se quedaba «calculando»). `party_strength` calcula el poder de verdad (antes 1000 fijo) (#69).
  - La misión de novato «Raise 1 character's Affinity to Lv. 3» (`member_friendlv`) ya cuenta la afinidad de los personajes (#65).
  - Herrería: «Crafting Complete» sin fabricar nada. El login sumaba cantidades fijas de las capturas originales (materiales 1100 ×3, 5001 ×4, 151 ×1, ticket 17 ×2 y tickets de jefe 230831 ×3), así que el juego creía tener más de lo que hay en la base de datos. Quitadas (#65).
  - Battle Arena EX (`battle_arena_ex.rs`, #72): se abre la temporada 2010 (la última que hubo) durante 30 días. 3 intentos al día; cada combate da un lote al azar del rango de su puntuación (S, A…, maestros `scorechallenge_ex_score*`) y suma al «achievement score» de la temporada, que da los premios por hito. «Skip» repite la mejor puntuación con los intentos que queden. El total de la temporada se guarda en `user_score_challenge_best` con el id de temporada en negativo (sin migración). El cliente no tiene clases de petición para estas llamadas: los parámetros se leen por nombre con alternativas y se apuntan en el log.
  - Puntuación de Arena EX (#73): el juego no manda la puntuación, sino `total_hit_damage`, `physical_hit_damage`, `magical_hit_damage`, `total_received_damage`, `battletime` (segundos de juego, de 60) y `survivor_num`. La calcula el servidor: base = daño × `difficulty_coefficient` × bono de tiempo (1 + segundos/60); bono según `bonus_type` de la fase (1 Defensa: 125000 × 1000 / max(daño recibido, 1000) × dificultad × bono de tiempo, con constantes del cliente, #74; 2/3 daño físico/mágico × dificultad), por el bono de supervivencia (+0,1 por superviviente). Las tablas oficiales de tiempo y supervivencia no existen en los datos: los números son nuestros.
  - Skip de Arena EX (#74): el juego manda `{"skip_count", "scorechallenge_ex_id"}` y ahora se cobra un Skip Ticket por intento saltado (antes el juego enseñaba 978 → 977 pero la base de datos no lo restaba).
  - Eventos activos configurables (`config/events.toml`, #75): pueden ir varios a la vez, como en el juego original. Cada uno tiene sus fechas, su canje, sus misiones y su banner de portada, que va el primero del carrusel solo. Por defecto sigue 24011; la lista de candidatos está en el fichero.
  - Misiones de Arena por elemento (#76): `scorechallenge_mission` y `scorechallenge_mission_list` muestran el mejor total de Arena de cada elemento (el del jefe, `attr1` de la fase) y las misiones logradas (250 000 a 10 M). Al superar una meta en un combate se dan sus premios (sin salir en la pantalla de resultado).
  - Segundo evento activo: 24041 «A Service For This Forbidden Weakest Adventurer!» (#77). Solo 5 eventos conservan su pantalla en el cliente (23121, 24011, 24012, 24013 y 24041); el 23111 daba «Data Load Failed». Al 24041 le faltan el banner y el cartel del canje en los recursos descargados (salen en blanco).
  - Banners que solo existen en otro idioma (#78): si falta `banners/EN/<fichero>`, el servidor lo busca en JP y KO antes que en el espejo remoto. El banner y el cartel de canje del 24041 solo están en coreano. Un fallo del espejo remoto ya no tumba la petición (404).
  - Misiones de Arena: ya no salen avisos de «Arena Best Score Updated» con 0 al abrirlas (`display_best_score_info` vacío) (#77).
  - Mazmorra: al sustituir una partida por otra de otra área, `started_at` pasa a ser la hora nueva (#76).
  - Fame Bonus (pociones de fama, `src/api/fame.rs`, #79; **necesita la migración 20261126000001**, sin ella todo sigue igual y el log avisa): al ganar una Fame Quest cada miembro de delante tiene un 20 % (`fame_bonus_incidence`) por carrera de sacar una poción, segura con cada Fame Supplement usado (se cobran al empezar). Cada poción trae 3 opciones distintas sorteadas con `fame_random_potion_lot`: +1 rango de PV, Atq. F., Atq. M., Def. F. o Def. M., o un rasgo (`skill_pa_fame`, rareza según `skill_pa_fame_lot_rare` y el rango de la misión; solo genéricos o del mismo personaje). El cliente manda los rangos nuevos y el servidor los guarda (máximo 25 por estadística). En las Fame Quests cada rango suma 100 PV o 20 al resto (`fame_quest_status_*`). Los rasgos son de cada personaje (hasta 30), se bloquean, se tiran y se ponen en la ranura de rasgo de cada formación. Pociones sin elegir: `fame_potion_list`. Pergamino de fama: da un rasgo al azar. Los nombres de algunos parámetros no están en el volcado del cliente: el servidor los busca por contenido y apunta cada petición en el log. Los rasgos salen sin estadísticas añadidas (no hay datos de cuáles traían).
  - Fame Potions probadas en el juego el 27/11 (#83): el cliente confirma con `fame_quest_potion_confirm {"confirm_potion_list": "[{user_member_id, selected_potion_info, fame_*_rank (total nuevo), discard_user_skill_pa_fame_id}]"}` y no llama a `fame_potion_list`, `confirm_potion_list` ni `skill_pa_fame_list`. Arreglos: un rasgo puesto en una formación dejaba Team → Party cargando para siempre, porque el cliente lee el primer `add_status` del rasgo y la lista iba vacía (ahora cada rasgo lleva un rango de una estadística al azar, y los viejos uno según su id); salía «Fame Rank UP!» tras cada misión (`fame_rank_up` ahora 0, el rango de fama de la cuenta no se lleva); los Fame Supplements daban poción segura a los 5 de la formación, ahora solo a los de delante (formaciones 1-3; los de detrás, 20 %).
  - **Rasgo de fama en la formación desactivado en las pantallas de equipo** (#84): con un rasgo puesto, Team → Party seguía en «Now Loading» incluso con la estadística añadida de #83. El rasgo se guarda, pero `partyinfo` y los combates mandan `skill_pa_fame` = 0 hasta encontrar la causa (`send_party_traits` y `trait_status` en `config/fame-quest.toml`, #85, para probar qué espera el cliente).
  - Poder que piden las Fame Quests (#82): en el original la fase más fácil pide 12.000 (las demás 15.000 o más) y el juego no deja pulsar «Begin» por debajo. Nuestro cálculo de poder coincide con las capturas, así que el bloqueo es real. `config/fame-quest.toml` (`required_power_percent`, 100 = original) permite rebajarlo; Pablo prefiere bajarlo solo para probar y volver a 100.
  - Suggest con la casilla de rasgo de fama (#80): pone en cada formación el rasgo más raro de su miembro principal. Si un rasgo deja de ser del principal de su formación (porque Suggest cambió el miembro), se quita.
  - Fame Quests con progreso (#86): cada victoria se guarda en `user_quest_progress` (como las demás misiones, sin migración) con sus coronas, que salen en la lista de fases. La primera vez que se logra cada corona da su premio (`fame_quest_reward`, lotes de `fame_quest_reward_item_list`) en la pantalla de resultado. El rango de fama de la cuenta ya no es un 5 fijo: sube al superar la misión de cada rango (`fame_rank.unlock_quest`) y entonces sale «Fame Rank UP!»; nunca baja de `min_fame_rank` (5) en `config/fame-quest.toml`. El cliente manda `clear_mission_list`; se aceptan números de corona (1-3) o ids de misión.
  - **Rasgo de fama en la formación arreglado** (#87): el cliente busca el rasgo de cada formación entre los objetos del jugador de tipo 43 (`UserParam.GetUnique(43, skill_pa_fame)`); si no lo encuentra, usa el número como id del maestro y, al no existir, salta una excepción que deja Team → Party en «Now Loading» (visto leyendo el código ARM64 del cliente). Ahora el login manda cada rasgo como dato de tipo 43 (`uniqid` = id propio, `item_id` = id del maestro), y también al ganar, tirar o usar un pergamino. `send_party_traits` vuelve a estar activado.
  - En combate el rasgo va con su id del maestro (#89): el combate no mira los datos de tipo 43, sino `SkillPaFameMaster` directamente, y con el id propio se colgaba al cargar (probado el 28/11 con una Free Quest). `partyinfo` sigue mandando el id propio y los `*start` (`BasicBattlePartyForm`) el del maestro.
  - Trial of the Ancients y Potential Blessing mandan el personaje como actualización (`UserParamUpdate`) en vez de como alta, para que la ficha se refresque sin reiniciar (#87, por probar).
  - Cartel de misión recomendada del evento con «Done» sin hacer nada: el cliente enseña la primera fila de `event_mission_recommend` que tenga una notificación de tipo 10 (`key` = recommend_id, `value` = mission_id) y, si no hay ninguna, la última con «Done». El login mandaba las de dos eventos viejos de la captura. Ahora manda, por cada evento activo, la de mayor prioridad aún sin cobrar, y lo mismo con la misión recomendada de la portada (tipo 6, `mission_recommend`) (#88).
  - Cambiar miembro en el equipo (`partychangelist`) manda ya la lista de miembros (antes iba vacía) (#86).
  - Los retos de mazmorra (`DungeonChallenge`) se dejan sin gastar a propósito: la respuesta de empezar zona no lleva contador y no sabemos cómo los descuenta el cliente; con intentos ilimitados se juega igual.
- **Copias y arranque** (#81): `scripts/konofan/copia-bd.sh [etiqueta]` guarda la base de datos en `~/backups` (las automáticas se rotan, se quedan 14; las que llevan etiqueta no se borran). `scripts/konofan/restaurar-bd.sh <copia>` la devuelve a una copia: con Axel parado, tras copiar el estado actual y pidiendo que escribas RESTAURAR. `scripts/konofan/arrancar.sh` arranca Axel en segundo plano, con una copia automática si hoy no hay ninguna, y espera a que diga que ha arrancado (registro en `logs/`).
- **Instalación local en Windows** (rama `instalacion-windows-mumu`): `scripts/konofan/setup-wsl.sh`, `scripts/konofan/mirror-assets.sh` y la guía [docs/konofan/INSTALAR-WINDOWS-MUMU.md](docs/konofan/INSTALAR-WINDOWS-MUMU.md). Probado de punta a punta en Ubuntu 24.04 (PostgreSQL 16, nginx, TLS), sin el juego.

## Qué falta

- El **+ de las gemas** se queda cargando: el juego espera a Google Play Billing y la petición nunca llega al servidor. Solo se arregla en el cliente; de momento, gemas por el buzón.
- Sin comprobar en el juego: que el cliente acepte rango y experiencia absolutos tras el combate (se mandan como las monedas) y que las fases bloqueadas se oculten bien.
- Battle Arena EX: probada en el juego (combates y Skip). El bono de defensa pesa mucho en la puntuación; quizá convenga escalarlo con los supervivientes. Los retos de mazmorra (`DungeonChallenge`) se compran pero no se gastan (intentos ilimitados).
- Endpoints todavía en stub (~100 avisos `encountered stub` en el código): sorteo de eventos, compras que fallan por el cliente, vincular cuenta, etc.
- Transfer ID en stub. Room Invitation sale vacía (no hay más jugadores). 
- Herramientas propias: **panel web** (Pablo lo quiere) para ver jugadores, tiradas y registros, y editar banners, probabilidades, eventos y personajes.

## Fallos encontrados jugando (2026-11-12)

| # | Fallo | Causa | Arreglo |
|---|-------|-------|---------|
| 1 | La portada no muestra el carrusel de eventos ni el botón Events | Los banners de la portada salen del maestro `banner` (1364 filas con `start_at`/`end_at`) y todos caducaron al cerrar el juego. Tampoco hay eventos activos (solo se reactiva el 24011 en `event_config`) | Parchear `banner`/`event_config` en `master_all.rs` para activar los que elijamos (después, desde el panel) |
| 2 | Regalos: «6» notificaciones, un regalo fijo de 10 000 gemas («skebob») y «Unable to receive: Stamina ×500» | `src/api/present.rs` es un stub: devuelve siempre los mismos datos de ejemplo de Axel y no entrega nada | Buzón de regalos real (tabla, entregar objetos); útil también para el panel |
| 3 | Note (avisos) en blanco | Dos causas: (a) `static_server.rs` usaba la ruta de la URL en `ServeDir::new` en vez de la carpeta local, así que todo salía del espejo (y `help/` devolvía su listado «Eris»); (b) el WebView (Chromium) rechazaba el certificado autofirmado que hacía de CA y de servidor | **Arreglado** (2026-11-12): `ServeDir::new(local_root)` (en el PR del hilo de fallos) y CA propia (`5ad9c253.0` en MuMu) + certificado de servidor de 397 días. Confirmado en el juego |
| 4 | Help de Branch en blanco | Igual que el 3 | **Arreglado** con el 3 |
| 5 | El «+» de gemas se queda cargando | No llega ninguna petición al servidor: el cliente espera a Google Play Billing, que en MuMu no responde | Baja prioridad: en local las gemas se dan por regalos o panel |

Otros stubs vistos en el registro: misiones (`mission_home`; `mission_list` ya es real), mini eventos sorpresa, recompensas por anuncios, invitaciones multijugador, vincular cuenta (`idlink_confirm_google`), intercambio (`exchange`) y propuesta de equipo (`party_offer`).

## Decisiones tomadas

- **2026-11-11 — Axel como base (fork), no solo como referencia.** Ya resuelve el protocolo, la firma JWT RS256 (RSA-1024) y LIAPP; empezar de cero repetiría meses de ingeniería inversa.
- **2026-11-11 — Servidor en WSL2 (Ubuntu 24.04) con red en espejo.** Axel y sus scripts están pensados para Linux; con `networkingMode=mirrored` el puerto 443 de WSL se ve desde Windows y MuMu sin redirecciones.
- **2026-11-12 — MuMu entra por `10.0.2.2`** (el localhost de Windows visto desde el emulador), no por la IP de la LAN: con la red en espejo de WSL, Windows no reenvía a WSL el tráfico hacia su propia IP.
- **2026-11-11 — Dominio `konofan.local` y certificado propio instalado como certificado de sistema en MuMu.** El cliente (Android 12+, OkHttp) no confía en certificados de usuario.
- **2026-11-12 — CA propia que firma el certificado del servidor (397 días).** El certificado inicial hacía de CA y de servidor a la vez: el juego lo aceptaba, pero Chromium y el WebView (Note, Help) lo rechazaban en silencio. En MuMu solo se instala la CA, así que renovar el certificado del servidor no obliga a tocar el emulador.
- **2026-11-11 — Recursos:** de momento se sirven desde el espejo de Assasans (`upstream-url`); `mirror-assets.sh` hace la copia local (~16 GiB) para no depender de él.
- La clave RSA de firma (`key.pem`) es la pública del repo de Axel; para uso local da igual. Si algún día se comparte el servidor, generar una propia (ver `SERVER-SETUP.md`).
- **2026-11-12 — Banners de portada y anuncios configurables desde fuera del código** (`config/home-banners.toml`, tablas `news` y `user_presents`), pensando en el futuro panel web.

## Recursos externos

- Espejo de Assasans: <https://smb.assasans.dev/konofd/> (APKs 3.8.7–4.11.6, bundles, banners, `gacha_infos/gacha_rates`, `global-metadata` descifrado). rsync: `rsync://aqua.assasans.dev/konofd/`.
- Cliente stub: <https://github.com/Assasans/konofd-client-stub/releases>.

## Siguientes pasos

1. Pablo: seguir la guía de instalación y llegar al menú principal en MuMu.
2. Probar una tirada en un banner con probabilidades importadas y revisar el log `gacha draw`.
3. ~~Pasar los banners de `gacha_info()` a la base de datos.~~ Hecho (#64, `gacha.banners`).
4. Editor de banners y probabilidades.

//! The shared tag vocabulary that lets taste in one medium inform the others.
//!
//! TMDB keywords and genres, IGDB themes/genres/keywords, Steam store tags and Last.fm /
//! MusicBrainz tags are all mapped into the same small set of features:
//!
//! * `g:<genre>`  a genre, normalized ("Science Fiction", "Sci-Fi & Fantasy" and IGDB's
//!   "Science fiction" all become `g:sci-fi`)
//! * `t:<tag>`    a mood / theme / tone tag that means the same thing in every medium
//!   (`t:cyberpunk`, `t:cozy`, `t:melancholic`, `t:slow-burn`...)
//!
//! A term can map to several features, and unknown terms are dropped rather than guessed at.

use crate::model::{slug, Creator, ItemDraft, Media};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Every shared mood/theme tag, with a short human label used in "why" lines and the Taste screen.
pub const TAGS: &[(&str, &str)] = &[
    ("cyberpunk", "cyberpunk"),
    ("space", "space"),
    ("dystopian", "dystopian worlds"),
    ("post-apocalyptic", "post-apocalyptic settings"),
    ("time-travel", "time travel"),
    ("ai", "artificial intelligence"),
    ("aliens", "first contact"),
    ("noir", "noir"),
    ("psychological", "psychological stories"),
    ("slow-burn", "slow burns"),
    ("melancholic", "melancholy"),
    ("cozy", "cozy vibes"),
    ("dark", "dark tone"),
    ("atmospheric", "thick atmosphere"),
    ("retro-synth", "retro synths"),
    ("epic", "epic scale"),
    ("mystery", "mysteries"),
    ("heist", "heists"),
    ("survival", "survival stories"),
    ("coming-of-age", "coming-of-age stories"),
    ("surreal", "the surreal"),
    ("dreamy", "dreamy moods"),
    ("gritty", "grit"),
    ("whimsical", "whimsy"),
    ("funny", "comedy"),
    ("satire", "satire"),
    ("romantic", "romance"),
    ("tense", "tension"),
    ("violent", "violence"),
    ("supernatural", "the supernatural"),
    ("gothic", "gothic style"),
    ("medieval", "medieval settings"),
    ("mythic", "myth and legend"),
    ("western", "westerns"),
    ("war", "war stories"),
    ("political", "politics"),
    ("crime", "crime stories"),
    ("detective", "detective work"),
    ("espionage", "spies"),
    ("martial-arts", "martial arts"),
    ("superhero", "superheroes"),
    ("anime", "anime style"),
    ("historical", "history"),
    ("nature", "the natural world"),
    ("ocean", "the sea"),
    ("road-trip", "road trips"),
    ("urban", "city life"),
    ("small-town", "small towns"),
    ("family", "family stories"),
    ("friendship", "friendship"),
    ("revenge", "revenge"),
    ("redemption", "redemption arcs"),
    ("underdog", "underdogs"),
    ("exploration", "exploration"),
    ("puzzle", "puzzles"),
    ("open-world", "open worlds"),
    ("narrative", "strong storytelling"),
    ("choices", "choices that matter"),
    ("challenging", "a challenge"),
    ("relaxing", "relaxing pace"),
    ("minimalist", "minimalism"),
    ("experimental", "experiments"),
    ("nostalgic", "nostalgia"),
    ("energetic", "high energy"),
    ("aggressive", "aggression"),
    ("uplifting", "uplift"),
    ("introspective", "introspection"),
    ("ethereal", "ethereal sounds"),
    ("groovy", "groove"),
    ("lo-fi", "lo-fi textures"),
    ("orchestral", "orchestral scores"),
    ("acoustic", "acoustic sounds"),
    ("electronic", "electronics"),
    ("jazzy", "jazz feel"),
    ("soulful", "soul"),
    ("rebellious", "rebellion"),
    ("philosophical", "big ideas"),
    ("existential", "existential themes"),
    ("tragic", "tragedy"),
    ("hopeful", "hope"),
    ("stylized", "strong visual style"),
    ("ensemble", "ensemble casts"),
    ("anthology", "anthologies"),
    ("true-story", "true stories"),
    ("music", "stories about music"),
    ("sports", "sports"),
    ("racing", "racing"),
    ("horror", "horror"),
    ("monsters", "monsters"),
    ("vampires", "vampires"),
    ("zombies", "zombies"),
    ("magic", "magic"),
    ("robots", "robots"),
    ("virtual-reality", "virtual worlds"),
    ("corporate", "corporate intrigue"),
    ("isolation", "isolation"),
    ("conspiracy", "conspiracies"),
    ("multiplayer", "playing together"),
    ("co-op", "co-op"),
    ("roguelike", "roguelikes"),
    ("soulslike", "soulslikes"),
    ("metroidvania", "metroidvanias"),
    ("pixel-art", "pixel art"),
    ("stealth", "stealth"),
    ("tactical", "tactics"),
    ("building", "building things"),
    ("farming", "farm life"),
    ("short", "short experiences"),
];

/// source term -> features. Terms are matched after `normalize`.
const ALIASES: &[(&str, &str)] = &[
    // ---- genres shared by film and TV (TMDB) ----
    ("action", "g:action t:energetic"),
    ("adventure", "g:adventure t:exploration"),
    ("action & adventure", "g:action g:adventure t:energetic"),
    ("animation", "g:animation t:stylized"),
    ("comedy", "g:comedy t:funny"),
    ("crime", "g:crime t:crime"),
    ("documentary", "g:documentary t:true-story"),
    ("drama", "g:drama"),
    ("family", "g:family t:family"),
    ("kids", "g:family t:family t:whimsical"),
    ("fantasy", "g:fantasy t:magic"),
    ("history", "g:history t:historical"),
    ("horror", "g:horror t:horror t:dark t:tense"),
    ("music", "g:music t:music"),
    ("mystery", "g:mystery t:mystery"),
    ("romance", "g:romance t:romantic"),
    ("science fiction", "g:sci-fi"),
    ("sci fi", "g:sci-fi"),
    ("scifi", "g:sci-fi"),
    ("sci fi & fantasy", "g:sci-fi g:fantasy"),
    ("tv movie", ""),
    ("thriller", "g:thriller t:tense"),
    ("war", "g:war t:war"),
    ("war & politics", "g:war t:war t:political"),
    ("western", "g:western t:western"),
    ("reality", "g:reality"),
    ("news", ""),
    ("talk", ""),
    ("soap", "g:drama t:romantic"),
    // ---- game genres (IGDB / Steam) ----
    ("role playing (rpg)", "g:rpg t:narrative"),
    ("role playing", "g:rpg t:narrative"),
    ("rpg", "g:rpg t:narrative"),
    ("jrpg", "g:rpg t:narrative t:anime"),
    ("action rpg", "g:rpg g:action"),
    ("shooter", "g:shooter t:energetic t:violent"),
    ("fps", "g:shooter t:energetic t:violent"),
    ("first person shooter", "g:shooter t:energetic t:violent"),
    ("platform", "g:platformer"),
    ("platformer", "g:platformer"),
    ("puzzle", "g:puzzle t:puzzle"),
    ("racing", "g:racing t:racing t:energetic"),
    ("real time strategy (rts)", "g:strategy t:tactical"),
    ("turn based strategy (tbs)", "g:strategy t:tactical"),
    ("strategy", "g:strategy t:tactical"),
    ("tactical", "g:strategy t:tactical"),
    ("simulator", "g:simulation"),
    ("simulation", "g:simulation"),
    ("sport", "g:sports t:sports"),
    ("sports", "g:sports t:sports"),
    ("fighting", "g:fighting t:martial-arts t:energetic"),
    ("hack and slash/beat 'em up", "g:action t:violent t:energetic"),
    ("hack and slash", "g:action t:violent t:energetic"),
    ("point and click", "g:adventure t:puzzle t:narrative"),
    ("visual novel", "g:visual-novel t:narrative t:choices"),
    ("indie", "g:indie"),
    ("arcade", "g:arcade t:nostalgic"),
    ("card & board game", "g:card-game t:tactical"),
    ("card game", "g:card-game t:tactical"),
    ("deckbuilding", "g:card-game t:tactical t:roguelike"),
    ("moba", "g:strategy t:multiplayer"),
    ("mmo", "g:rpg t:multiplayer t:open-world"),
    ("massively multiplayer", "g:rpg t:multiplayer t:open-world"),
    ("music game", "g:rhythm t:music"),
    ("rhythm", "g:rhythm t:music"),
    ("pinball", "g:arcade"),
    ("quiz/trivia", "g:puzzle"),
    ("casual", "t:relaxing"),
    ("free to play", ""),
    ("early access", ""),
    // ---- game themes and store tags ----
    ("open world", "t:open-world t:exploration"),
    ("sandbox", "t:open-world t:building"),
    ("stealth", "t:stealth t:tense"),
    ("survival", "t:survival t:tense"),
    ("survival horror", "g:horror t:horror t:survival t:tense"),
    ("4x (explore, expand, exploit, and exterminate)", "g:strategy t:tactical t:epic"),
    ("business", "g:simulation t:corporate"),
    ("educational", ""),
    ("erotic", ""),
    ("non fiction", "t:true-story"),
    ("party", "t:multiplayer t:funny"),
    ("warfare", "t:war"),
    ("roguelike", "t:roguelike t:challenging"),
    ("roguelite", "t:roguelike t:challenging"),
    ("rogue like", "t:roguelike t:challenging"),
    ("rogue lite", "t:roguelike t:challenging"),
    ("souls like", "t:soulslike t:challenging t:dark"),
    ("soulslike", "t:soulslike t:challenging t:dark"),
    ("metroidvania", "t:metroidvania t:exploration"),
    ("pixel graphics", "t:pixel-art t:nostalgic"),
    ("pixel art", "t:pixel-art t:nostalgic"),
    ("retro", "t:nostalgic"),
    ("story rich", "t:narrative"),
    ("narrative", "t:narrative"),
    ("choices matter", "t:choices t:narrative"),
    ("multiple endings", "t:choices"),
    ("walking simulator", "t:narrative t:atmospheric t:slow-burn t:short"),
    ("exploration", "t:exploration"),
    ("atmospheric", "t:atmospheric"),
    ("difficult", "t:challenging"),
    ("relaxing", "t:relaxing t:cozy"),
    ("cozy", "t:cozy t:relaxing"),
    ("wholesome", "t:cozy t:uplifting"),
    ("cute", "t:cozy t:whimsical"),
    ("farming sim", "g:simulation t:farming t:cozy t:relaxing"),
    ("farming", "t:farming t:cozy"),
    ("life sim", "g:simulation t:cozy t:relaxing"),
    ("city builder", "g:simulation t:building"),
    ("building", "t:building"),
    ("base building", "t:building t:survival"),
    ("crafting", "t:building t:survival"),
    ("co op", "t:co-op t:multiplayer"),
    ("online co op", "t:co-op t:multiplayer"),
    ("local co op", "t:co-op t:multiplayer"),
    ("multiplayer", "t:multiplayer"),
    ("pvp", "t:multiplayer t:challenging"),
    ("singleplayer", ""),
    ("single player", ""),
    ("short", "t:short"),
    ("minimalist", "t:minimalist"),
    ("stylized", "t:stylized"),
    ("hand drawn", "t:stylized"),
    ("turn based", "t:tactical"),
    ("turn based combat", "t:tactical"),
    ("immersive sim", "t:choices t:stealth t:atmospheric"),
    ("psychological horror", "g:horror t:horror t:psychological t:dark"),
    ("lovecraftian", "t:horror t:supernatural t:existential t:dark"),
    ("cosmic horror", "t:horror t:supernatural t:existential t:dark"),
    // ---- settings and themes (all media) ----
    ("cyberpunk", "t:cyberpunk t:dystopian t:urban t:retro-synth"),
    ("neo noir", "t:noir t:dark t:stylized t:urban"),
    ("film noir", "t:noir t:dark t:crime"),
    ("noir", "t:noir t:dark"),
    ("tech noir", "t:noir t:cyberpunk t:dark"),
    ("dystopia", "t:dystopian"),
    ("dystopian", "t:dystopian"),
    ("post apocalyptic", "t:post-apocalyptic t:survival"),
    ("post apocalyptic future", "t:post-apocalyptic t:survival"),
    ("apocalypse", "t:post-apocalyptic"),
    ("space", "t:space"),
    ("outer space", "t:space"),
    ("space travel", "t:space t:exploration"),
    ("space opera", "t:space t:epic"),
    ("spacecraft", "t:space"),
    ("space station", "t:space t:isolation"),
    ("astronaut", "t:space t:isolation"),
    ("alien", "t:aliens"),
    ("aliens", "t:aliens"),
    ("alien contact", "t:aliens t:philosophical"),
    ("first contact", "t:aliens t:philosophical"),
    ("alien invasion", "t:aliens t:war"),
    ("extraterrestrial", "t:aliens"),
    ("time travel", "t:time-travel"),
    ("time loop", "t:time-travel t:puzzle"),
    ("artificial intelligence", "t:ai"),
    ("artificial intelligence (a.i.)", "t:ai"),
    ("android", "t:ai t:robots"),
    ("robot", "t:robots"),
    ("robots", "t:robots"),
    ("mecha", "t:robots t:anime"),
    ("virtual reality", "t:virtual-reality"),
    ("simulated reality", "t:virtual-reality t:philosophical"),
    ("hacker", "t:cyberpunk t:conspiracy"),
    ("hacking", "t:cyberpunk"),
    ("megacorporation", "t:corporate t:cyberpunk"),
    ("corporate", "t:corporate"),
    ("conspiracy", "t:conspiracy t:tense"),
    ("government conspiracy", "t:conspiracy t:political"),
    ("surveillance", "t:conspiracy t:dystopian"),
    ("psychological thriller", "t:psychological t:tense"),
    ("psychological", "t:psychological"),
    ("psychological drama", "t:psychological t:introspective"),
    ("mind bending", "t:psychological t:surreal"),
    ("mindfuck", "t:psychological t:surreal"),
    ("unreliable narrator", "t:psychological t:mystery"),
    ("memory", "t:psychological t:introspective"),
    ("identity", "t:introspective t:existential"),
    ("dream", "t:dreamy t:surreal"),
    ("dreams", "t:dreamy t:surreal"),
    ("surreal", "t:surreal"),
    ("surrealism", "t:surreal"),
    ("slow burn", "t:slow-burn"),
    ("slow cinema", "t:slow-burn t:minimalist"),
    ("slow paced", "t:slow-burn"),
    ("meditative", "t:slow-burn t:introspective"),
    ("contemplative", "t:slow-burn t:introspective"),
    ("philosophical", "t:philosophical"),
    ("philosophy", "t:philosophical"),
    ("existentialism", "t:existential t:philosophical"),
    ("existential", "t:existential"),
    ("loneliness", "t:isolation t:melancholic"),
    ("isolation", "t:isolation"),
    ("grief", "t:melancholic t:tragic"),
    ("loss", "t:melancholic"),
    ("melancholy", "t:melancholic"),
    ("melancholic", "t:melancholic"),
    ("melancholia", "t:melancholic"),
    ("bittersweet", "t:melancholic t:hopeful"),
    ("sad", "t:melancholic"),
    ("depression", "t:melancholic t:introspective"),
    ("tragedy", "t:tragic"),
    ("hopeful", "t:hopeful t:uplifting"),
    ("feel good", "t:uplifting t:cozy"),
    ("heartwarming", "t:uplifting t:cozy"),
    ("inspirational", "t:uplifting t:underdog"),
    ("dark", "t:dark"),
    ("dark fantasy", "g:fantasy t:dark t:gothic"),
    ("dark comedy", "g:comedy t:dark t:satire"),
    ("black comedy", "g:comedy t:dark t:satire"),
    ("satire", "t:satire t:funny"),
    ("parody", "t:satire t:funny"),
    ("absurd", "t:surreal t:funny"),
    ("gritty", "t:gritty"),
    ("gothic", "t:gothic t:dark"),
    ("gothic horror", "t:gothic t:horror t:dark"),
    ("supernatural", "t:supernatural"),
    ("paranormal", "t:supernatural"),
    ("ghost", "t:supernatural t:horror"),
    ("haunted house", "t:supernatural t:horror t:gothic"),
    ("occult", "t:supernatural t:dark"),
    ("witch", "t:supernatural t:magic"),
    ("magic", "t:magic"),
    ("wizard", "t:magic"),
    ("sword and sorcery", "g:fantasy t:magic t:medieval"),
    ("high fantasy", "g:fantasy t:magic t:epic"),
    ("epic fantasy", "g:fantasy t:magic t:epic"),
    ("dragon", "g:fantasy t:monsters t:mythic"),
    ("mythology", "t:mythic"),
    ("norse mythology", "t:mythic"),
    ("greek mythology", "t:mythic"),
    ("folklore", "t:mythic"),
    ("fairy tale", "t:mythic t:whimsical"),
    ("medieval", "t:medieval"),
    ("middle ages", "t:medieval t:historical"),
    ("knight", "t:medieval"),
    ("samurai", "t:martial-arts t:historical"),
    ("ninja", "t:martial-arts t:stealth"),
    ("martial arts", "t:martial-arts"),
    ("kung fu", "t:martial-arts"),
    ("monster", "t:monsters"),
    ("kaiju", "t:monsters t:epic"),
    ("creature", "t:monsters"),
    ("vampire", "t:vampires t:gothic t:supernatural"),
    ("vampires", "t:vampires t:gothic t:supernatural"),
    ("zombie", "t:zombies t:horror t:survival"),
    ("zombies", "t:zombies t:horror t:survival"),
    ("werewolf", "t:monsters t:supernatural"),
    ("slasher", "t:horror t:violent"),
    ("gore", "t:violent t:horror"),
    ("body horror", "t:horror t:surreal t:dark"),
    ("found footage", "t:horror t:tense"),
    ("folk horror", "t:horror t:mythic t:small-town"),
    ("superhero", "t:superhero"),
    ("based on comic", "t:superhero t:stylized"),
    ("marvel cinematic universe (mcu)", "t:superhero t:epic"),
    ("anime", "t:anime t:stylized"),
    ("based on manga", "t:anime"),
    ("heist", "t:heist t:crime"),
    ("con artist", "t:heist t:crime"),
    ("gangster", "t:crime t:violent"),
    ("mafia", "t:crime t:violent t:family"),
    ("organized crime", "t:crime"),
    ("drug cartel", "t:crime t:violent"),
    ("serial killer", "t:crime t:psychological t:dark"),
    ("murder", "t:crime t:mystery"),
    ("murder mystery", "t:mystery t:detective"),
    ("whodunit", "t:mystery t:detective t:ensemble"),
    ("detective", "t:detective t:mystery"),
    ("private detective", "t:detective t:noir"),
    ("police", "t:crime t:detective"),
    ("investigation", "t:detective t:mystery"),
    ("true crime", "t:crime t:true-story"),
    ("courtroom", "t:crime t:political"),
    ("spy", "t:espionage"),
    ("espionage", "t:espionage"),
    ("secret agent", "t:espionage"),
    ("cold war", "t:espionage t:historical t:political"),
    ("assassin", "t:revenge t:violent"),
    ("hitman", "t:crime t:violent"),
    ("revenge", "t:revenge"),
    ("redemption", "t:redemption"),
    ("underdog", "t:underdog"),
    ("political", "t:political"),
    ("politics", "t:political"),
    ("political thriller", "t:political t:conspiracy t:tense"),
    ("world war ii", "t:war t:historical"),
    ("world war i", "t:war t:historical"),
    ("vietnam war", "t:war t:historical"),
    ("military", "t:war"),
    ("soldier", "t:war"),
    ("anti war", "t:war t:tragic"),
    ("historical", "t:historical"),
    ("period drama", "t:historical"),
    ("historical fiction", "t:historical"),
    ("based on true story", "t:true-story"),
    ("biography", "t:true-story"),
    ("biopic", "t:true-story"),
    ("based on novel or book", "t:narrative"),
    ("coming of age", "t:coming-of-age"),
    ("teenager", "t:coming-of-age"),
    ("high school", "t:coming-of-age"),
    ("friendship", "t:friendship"),
    ("buddy", "t:friendship t:funny"),
    ("found family", "t:friendship t:family"),
    ("father son relationship", "t:family"),
    ("mother daughter relationship", "t:family"),
    ("sibling relationship", "t:family"),
    ("love", "t:romantic"),
    ("romantic comedy", "g:comedy g:romance t:romantic t:funny t:cozy"),
    ("romcom", "g:comedy g:romance t:romantic t:funny t:cozy"),
    ("road trip", "t:road-trip"),
    ("road movie", "t:road-trip"),
    ("small town", "t:small-town"),
    ("new york city", "t:urban"),
    ("los angeles, california", "t:urban"),
    ("tokyo, japan", "t:urban"),
    ("city", "t:urban"),
    ("urban", "t:urban"),
    ("nature", "t:nature"),
    ("wilderness", "t:nature t:survival"),
    ("forest", "t:nature"),
    ("mountain", "t:nature"),
    ("ocean", "t:ocean t:nature"),
    ("sea", "t:ocean"),
    ("underwater", "t:ocean t:exploration"),
    ("pirate", "t:ocean t:exploration"),
    ("island", "t:ocean t:isolation"),
    ("desert", "t:nature t:isolation"),
    ("wild west", "t:western"),
    ("cowboy", "t:western"),
    ("neo western", "t:western t:gritty"),
    ("boxing", "t:sports t:underdog"),
    ("musical", "g:musical t:music"),
    ("musician", "t:music"),
    ("rock band", "t:music"),
    ("ensemble cast", "t:ensemble"),
    ("anthology", "t:anthology"),
    ("miniseries", "t:short"),
    ("limited series", "t:short"),
    ("workplace comedy", "g:comedy t:funny t:ensemble"),
    ("sitcom", "g:comedy t:funny t:cozy"),
    ("mockumentary", "g:comedy t:satire t:funny"),
    ("stand up comedy", "g:comedy t:funny"),
    ("epic", "t:epic"),
    ("visually stunning", "t:stylized"),
    ("experimental", "t:experimental"),
    ("avant garde", "t:experimental"),
    ("arthouse", "t:experimental t:slow-burn"),
    ("independent film", "g:indie"),
    ("cult film", "t:stylized"),
    ("nostalgia", "t:nostalgic"),
    ("nostalgic", "t:nostalgic"),
    ("1980s", "t:nostalgic t:retro-synth"),
    ("80s", "t:nostalgic t:retro-synth"),
    ("christmas", "t:cozy t:family"),
    ("holiday", "t:cozy"),
    ("cooking", "t:cozy"),
    ("food", "t:cozy"),
    ("rebellion", "t:rebellious"),
    ("resistance", "t:rebellious t:political"),
    ("revolution", "t:rebellious t:political"),
    ("punk", "g:punk t:rebellious t:aggressive t:energetic"),
    // ---- music genres and moods (Last.fm / MusicBrainz) ----
    ("rock", "g:rock"),
    ("classic rock", "g:rock t:nostalgic"),
    ("alternative", "g:alternative"),
    ("alternative rock", "g:alternative g:rock"),
    ("indie rock", "g:indie g:rock"),
    ("indie pop", "g:indie g:pop"),
    ("indie folk", "g:indie g:folk t:acoustic t:introspective"),
    ("pop", "g:pop"),
    ("synthpop", "g:pop g:electronic t:retro-synth t:electronic"),
    ("synth pop", "g:pop g:electronic t:retro-synth t:electronic"),
    ("dream pop", "g:pop t:dreamy t:ethereal t:atmospheric"),
    ("shoegaze", "g:rock t:dreamy t:ethereal t:atmospheric"),
    ("art pop", "g:pop t:experimental"),
    ("k pop", "g:pop t:energetic"),
    ("j pop", "g:pop t:anime"),
    ("hip hop", "g:hip-hop t:urban"),
    ("rap", "g:hip-hop t:urban"),
    ("trap", "g:hip-hop t:urban t:dark"),
    ("r&b", "g:r&b t:soulful t:groovy"),
    ("rnb", "g:r&b t:soulful t:groovy"),
    ("soul", "g:soul t:soulful"),
    ("neo soul", "g:soul t:soulful t:groovy"),
    ("funk", "g:funk t:groovy t:energetic"),
    ("disco", "g:disco t:groovy t:nostalgic"),
    ("jazz", "g:jazz t:jazzy"),
    ("jazz fusion", "g:jazz t:jazzy t:groovy"),
    ("blues", "g:blues t:soulful"),
    ("electronic", "g:electronic t:electronic"),
    ("electronica", "g:electronic t:electronic"),
    ("edm", "g:electronic t:electronic t:energetic"),
    ("house", "g:electronic t:electronic t:groovy"),
    ("techno", "g:electronic t:electronic t:dark t:energetic"),
    ("trance", "g:electronic t:electronic t:ethereal"),
    ("drum and bass", "g:electronic t:electronic t:energetic"),
    ("dubstep", "g:electronic t:electronic t:aggressive"),
    ("idm", "g:electronic t:electronic t:experimental"),
    ("ambient", "g:ambient t:atmospheric t:relaxing t:ethereal t:slow-burn"),
    ("dark ambient", "g:ambient t:atmospheric t:dark t:isolation"),
    ("drone", "g:ambient t:atmospheric t:minimalist t:experimental"),
    ("downtempo", "g:electronic t:relaxing t:atmospheric"),
    ("chillout", "g:electronic t:relaxing"),
    ("chill", "t:relaxing"),
    ("trip hop", "g:electronic t:atmospheric t:dark t:groovy t:noir"),
    ("lo fi", "t:lo-fi t:relaxing t:cozy"),
    ("lofi", "t:lo-fi t:relaxing t:cozy"),
    ("chillwave", "g:electronic t:dreamy t:nostalgic t:retro-synth"),
    ("vaporwave", "g:electronic t:nostalgic t:retro-synth t:surreal"),
    ("synthwave", "g:electronic t:retro-synth t:nostalgic t:cyberpunk t:electronic"),
    ("retrowave", "g:electronic t:retro-synth t:nostalgic t:cyberpunk t:electronic"),
    ("outrun", "g:electronic t:retro-synth t:nostalgic t:racing"),
    ("darksynth", "g:electronic t:retro-synth t:dark t:cyberpunk t:aggressive"),
    ("new wave", "g:rock t:retro-synth t:nostalgic"),
    ("post punk", "g:rock t:dark t:gothic t:rebellious"),
    ("darkwave", "g:electronic t:gothic t:dark t:retro-synth"),
    ("goth", "g:rock t:gothic t:dark"),
    ("industrial", "g:electronic t:aggressive t:dark t:dystopian"),
    ("metal", "g:metal t:aggressive t:energetic"),
    ("heavy metal", "g:metal t:aggressive t:energetic"),
    ("black metal", "g:metal t:aggressive t:dark t:atmospheric"),
    ("death metal", "g:metal t:aggressive t:violent"),
    ("doom metal", "g:metal t:dark t:slow-burn"),
    ("progressive metal", "g:metal t:epic t:experimental"),
    ("progressive rock", "g:rock t:epic t:experimental"),
    ("post rock", "g:rock t:atmospheric t:epic t:slow-burn t:melancholic"),
    ("post metal", "g:metal t:atmospheric t:epic t:slow-burn"),
    ("math rock", "g:rock t:experimental"),
    ("hardcore", "g:punk t:aggressive t:energetic"),
    ("emo", "g:rock t:melancholic t:coming-of-age"),
    ("pop punk", "g:punk g:pop t:energetic t:coming-of-age"),
    ("grunge", "g:rock t:gritty t:melancholic"),
    ("psychedelic", "t:surreal t:dreamy"),
    ("psychedelic rock", "g:rock t:surreal t:dreamy"),
    ("folk", "g:folk t:acoustic"),
    ("singer songwriter", "g:folk t:acoustic t:introspective"),
    ("acoustic", "t:acoustic"),
    ("country", "g:country t:western t:small-town"),
    ("americana", "g:country t:road-trip t:acoustic"),
    ("bluegrass", "g:country t:acoustic"),
    ("classical", "g:classical t:orchestral"),
    ("modern classical", "g:classical t:orchestral t:minimalist t:melancholic"),
    ("neoclassical", "g:classical t:orchestral t:melancholic"),
    ("orchestral", "t:orchestral t:epic"),
    ("soundtrack", "g:soundtrack t:orchestral t:atmospheric"),
    ("score", "g:soundtrack t:orchestral t:atmospheric"),
    ("video game music", "g:soundtrack t:nostalgic"),
    ("chiptune", "g:electronic t:pixel-art t:nostalgic"),
    ("reggae", "g:reggae t:relaxing t:groovy"),
    ("latin", "g:latin t:groovy"),
    ("afrobeat", "g:world t:groovy t:energetic"),
    ("world", "g:world"),
    ("experimental music", "t:experimental"),
    ("noise", "t:experimental t:aggressive"),
    ("minimal", "t:minimalist"),
    ("minimalism", "t:minimalist"),
    ("instrumental", "t:atmospheric"),
    ("ethereal", "t:ethereal"),
    ("dreamy", "t:dreamy"),
    ("mellow", "t:relaxing"),
    ("upbeat", "t:uplifting t:energetic"),
    ("happy", "t:uplifting"),
    ("energetic", "t:energetic"),
    ("aggressive", "t:aggressive"),
    ("angry", "t:aggressive"),
    ("introspective", "t:introspective"),
    ("haunting", "t:atmospheric t:dark t:ethereal"),
    ("cinematic", "t:epic t:atmospheric t:orchestral"),
    ("epic music", "t:epic"),
    ("groovy", "t:groovy"),
    ("soulful", "t:soulful"),
    ("romantic music", "t:romantic"),
    ("love songs", "t:romantic"),
    ("night", "t:noir t:atmospheric"),
    ("late night", "t:noir t:atmospheric t:relaxing"),
    ("rainy day", "t:cozy t:melancholic t:relaxing"),
    ("driving", "t:road-trip t:energetic"),
    ("summer", "t:uplifting"),
    ("winter", "t:cozy t:melancholic"),
];

fn alias_map() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static MAP: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    MAP.get_or_init(|| ALIASES.iter().map(|(k, v)| (*k, v.split_whitespace().collect())).collect())
}

fn tag_set() -> &'static HashMap<&'static str, &'static str> {
    static SET: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    SET.get_or_init(|| TAGS.iter().copied().collect())
}

/// Lowercases, turns punctuation that varies between services into spaces, and collapses runs.
pub fn normalize(term: &str) -> String {
    let lower = term.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut space = false;
    for c in lower.chars() {
        let c = match c {
            '-' | '_' => ' ',
            '’' => '\'',
            c => c,
        };
        if c == ' ' {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out.trim().to_string()
}

/// Maps one source term to shared features. Unknown terms return nothing.
pub fn map_term(term: &str) -> Vec<&'static str> {
    let n = normalize(term);
    if let Some(v) = alias_map().get(n.as_str()) {
        return v.clone();
    }
    // A term that is already a shared tag ("cozy", "slow-burn") maps to itself.
    let dashed = n.replace(' ', "-");
    if let Some((k, _)) = tag_set().get_key_value(dashed.as_str()) {
        // Leak-free: build the "t:<tag>" from the static table.
        return TAG_FEATURES.get_or_init(build_tag_features).get(*k).map(|f| vec![*f]).unwrap_or_default();
    }
    Vec::new()
}

static TAG_FEATURES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

fn build_tag_features() -> HashMap<&'static str, &'static str> {
    TAGS.iter().map(|(t, _)| (*t, &*Box::leak(format!("t:{t}").into_boxed_str()))).collect()
}

/// Human label for a feature, for "why" lines and the Taste screen.
pub fn label(feature: &str) -> String {
    let (kind, value) = feature.split_once(':').unwrap_or(("", feature));
    match kind {
        "t" => tag_set().get(value).map(|s| s.to_string()).unwrap_or_else(|| value.replace('-', " ")),
        "g" => match value {
            "sci-fi" => "sci-fi".into(),
            "rpg" => "RPGs".into(),
            "r&b" => "R&B".into(),
            "hip-hop" => "hip-hop".into(),
            v => v.replace('-', " "),
        },
        "e" => format!("the {value}"),
        "l" => language_name(value).to_string(),
        "n" => match value {
            "short" => "shorter runtimes".into(),
            "long" => "long runtimes".into(),
            _ => "mid-length runtimes".into(),
        },
        _ => value.replace('-', " "),
    }
}

pub fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "es" => "Spanish",
        "de" => "German",
        "it" => "Italian",
        "zh" | "cn" => "Chinese",
        "hi" => "Hindi",
        "sv" => "Swedish",
        "da" => "Danish",
        "no" => "Norwegian",
        "pt" => "Portuguese",
        "ru" => "Russian",
        other => other,
    }
}

fn creator_weight(role: &str) -> f64 {
    match role {
        "director" | "developer" | "artist" | "creator" => 1.0,
        "writer" => 0.7,
        "studio" | "network" => 0.45,
        "publisher" => 0.35,
        "label" => 0.3,
        "cast" => 0.4,
        "composer" => 0.5,
        _ => 0.3,
    }
}

fn length_bucket(media: Media, minutes: i64) -> Option<&'static str> {
    if minutes <= 0 {
        return None;
    }
    let (short, long) = match media {
        Media::Movie => (95, 140),
        Media::Tv => (32, 55),            // episode length
        Media::Game => (8 * 60, 40 * 60), // time to beat
        Media::Album => (32, 60),
        Media::Artist => return None,
    };
    Some(if minutes < short {
        "short"
    } else if minutes > long {
        "long"
    } else {
        "medium"
    })
}

/// Turns a draft into the weighted features the taste engine scores on.
pub fn features_for(d: &ItemDraft) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    let mut add = |f: String, w: f64| {
        let e = out.entry(f).or_insert(0.0);
        if w > *e {
            *e = w;
        }
    };
    for g in &d.genres {
        let mapped = map_term(g);
        if mapped.is_empty() && !normalize(g).is_empty() && alias_map().get(normalize(g).as_str()).is_none() {
            // A genre we have no alias for still counts as itself within its medium.
            add(format!("g:{}", slug(g)), 1.0);
        }
        for f in mapped {
            add(f.to_string(), if f.starts_with("g:") { 1.0 } else { 0.6 });
        }
    }
    // Earlier tags are the ones the source ranks as most relevant, so they weigh a little more.
    for (i, t) in d.tags.iter().enumerate() {
        let w = (1.0 - i as f64 * 0.02).max(0.5);
        for f in map_term(t) {
            add(f.to_string(), if f.starts_with("t:") { w } else { w * 0.8 });
        }
    }
    let mut cast_seen = 0;
    for Creator { name, role } in &d.creators {
        if name.trim().is_empty() {
            continue;
        }
        if role == "cast" {
            cast_seen += 1;
            if cast_seen > 4 {
                continue;
            }
        }
        add(format!("c:{}", slug(name)), creator_weight(role));
    }
    if let Some(y) = d.year.or_else(|| d.release_date.as_deref().and_then(crate::model::year_of)) {
        if (1900..2100).contains(&y) {
            add(format!("e:{}s", y / 10 * 10), 0.5);
        }
    }
    if let Some(l) = d.language.as_deref().filter(|l| !l.is_empty()) {
        add(format!("l:{}", l.to_lowercase()), 0.4);
    }
    if let Some(b) = d.runtime.and_then(|m| length_bucket(d.media(), m)) {
        add(format!("n:{b}"), 0.4);
        if b == "short" && matches!(d.media(), Media::Game) {
            add("t:short".into(), 0.6);
        }
    }
    out
}

/// Finds shared tags mentioned in free text ("a short cozy game for a rainy Sunday").
/// Used by vibe search when Ollama is off.
pub fn tags_in_text(text: &str) -> Vec<&'static str> {
    let n = format!(" {} ", normalize(text));
    let mut found: Vec<&'static str> = Vec::new();
    let mut keys: Vec<&&str> = alias_map().keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for k in keys {
        if k.len() < 3 {
            continue;
        }
        if n.contains(&format!(" {k} ")) {
            for f in &alias_map()[*k] {
                if !found.contains(f) {
                    found.push(f);
                }
            }
        }
    }
    for (t, _) in TAGS {
        let spaced = t.replace('-', " ");
        if n.contains(&format!(" {spaced} ")) || n.contains(&format!(" {t} ")) {
            if let Some(f) = TAG_FEATURES.get_or_init(build_tag_features).get(t) {
                if !found.contains(f) {
                    found.push(f);
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats(media: Media, genres: &[&str], tags: &[&str]) -> BTreeMap<String, f64> {
        let mut d = ItemDraft::new(media, "x");
        d.genres = genres.iter().map(|s| s.to_string()).collect();
        d.tags = tags.iter().map(|s| s.to_string()).collect();
        features_for(&d)
    }

    #[test]
    fn every_alias_points_at_a_known_tag() {
        for (term, mapped) in ALIASES {
            assert_eq!(normalize(term), *term, "alias key isn't normalized: {term}");
            for f in mapped.split_whitespace() {
                let (kind, value) = f.split_once(':').expect("feature has a kind");
                assert!(kind == "g" || kind == "t", "{term} -> {f}");
                if kind == "t" {
                    assert!(tag_set().contains_key(value), "{term} maps to unknown tag {value}");
                }
            }
        }
    }

    #[test]
    fn no_alias_is_listed_twice() {
        let mut seen = std::collections::HashSet::new();
        for (term, _) in ALIASES {
            assert!(seen.insert(term), "duplicate alias {term}");
        }
    }

    #[test]
    fn the_same_idea_from_three_services_lands_on_one_feature() {
        // TMDB keyword, IGDB theme, Last.fm tag.
        assert!(map_term("cyberpunk").contains(&"t:cyberpunk"));
        assert!(map_term("Science Fiction").contains(&"g:sci-fi"));
        assert!(map_term("Sci-Fi & Fantasy").contains(&"g:sci-fi"));
        assert!(map_term("synthwave").contains(&"t:retro-synth"));
        assert!(map_term("Synth-pop").contains(&"t:retro-synth"));
        assert!(map_term("Role-playing (RPG)").contains(&"g:rpg"));
        assert!(map_term("slow burn").contains(&"t:slow-burn"));
        assert!(map_term("slow-burn").contains(&"t:slow-burn"));
    }

    #[test]
    fn a_moody_scifi_film_shares_features_with_a_game_and_an_album() {
        let film = feats(Media::Movie, &["Science Fiction", "Drama"], &["alien contact", "melancholy", "slow burn"]);
        let game = feats(Media::Game, &["Adventure", "Indie"], &["space", "exploration", "atmospheric", "mystery"]);
        let album = feats(Media::Album, &[], &["ambient", "synthwave", "melancholic"]);
        let shared = |a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>| a.keys().filter(|k| k.starts_with("t:") && b.contains_key(*k)).count();
        assert!(shared(&film, &album) >= 2, "film/album share {:?} / {:?}", film.keys(), album.keys());
        assert!(shared(&game, &album) >= 1);
        assert!(film.contains_key("t:aliens") && film.contains_key("g:sci-fi"));
    }

    #[test]
    fn unknown_terms_are_dropped_not_guessed() {
        assert!(map_term("qwertyuiop").is_empty());
        assert!(map_term("").is_empty());
    }

    #[test]
    fn creators_era_language_and_length_become_features() {
        let mut d = ItemDraft::new(Media::Movie, "Arrival");
        d.year = Some(2016);
        d.language = Some("en".into());
        d.runtime = Some(116);
        d.creators = vec![Creator::new("Denis Villeneuve", "director"), Creator::new("Amy Adams", "cast")];
        let f = features_for(&d);
        assert_eq!(f["c:denis-villeneuve"], 1.0);
        assert_eq!(f["c:amy-adams"], 0.4);
        assert!(f.contains_key("e:2010s") && f.contains_key("l:en") && f.contains_key("n:medium"));
    }

    #[test]
    fn free_text_finds_tags() {
        let t = tags_in_text("a short cozy game for a rainy Sunday");
        assert!(t.contains(&"t:cozy") && t.contains(&"t:short"), "{t:?}");
        let t = tags_in_text("albums that sound like a cyberpunk city at night");
        assert!(t.contains(&"t:cyberpunk") && t.contains(&"t:noir"), "{t:?}");
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(label("t:slow-burn"), "slow burns");
        assert_eq!(label("g:sci-fi"), "sci-fi");
        assert_eq!(label("e:1980s"), "the 1980s");
    }
}

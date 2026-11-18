// Corpus report for the recompiler: which API each mod needs and how much of it we provide.
//   java Analyze.java <top500.tsv> <jar dir> <out dir> <provided jar|dir>...
// "Provided" = the fabric-shim classes, Brigadier, and TeaVM's class library (its
// org/teavm/classlib/.../TFoo classes stand for java/.../Foo). A reference is covered when its
// owner or one of the owner's provided supertypes declares the member.
import java.io.IOException;
import java.io.InputStream;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.FieldModel;
import java.lang.classfile.MethodModel;
import java.lang.classfile.constantpool.ClassEntry;
import java.lang.classfile.constantpool.MemberRefEntry;
import java.lang.classfile.constantpool.PoolEntry;
import java.lang.classfile.constantpool.Utf8Entry;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

public class Analyze {
    record Info(String superName, List<String> interfaces, Set<String> members) {}

    /** A class or member reference; {@code member} is "" for a bare class reference. */
    record Ref(String owner, String member) {
        String key() {
            return member.isEmpty() ? owner : owner + "." + member;
        }
    }

    static final class Mod {
        String rank, slug, file;
        final Set<String> own = new HashSet<>();
        final Set<Ref> refs = new HashSet<>();
        int classes, mixinClasses, nestedJars;
        boolean mixinConfig, serverEntrypoint, mainEntrypoint, clientEntrypoint;
        final Map<String, Set<String>> missing = new TreeMap<>(); // category -> missing keys
        final Map<String, Integer> external = new TreeMap<>(); // category -> distinct refs
        String error;
    }

    static final Map<String, Info> provided = new HashMap<>();
    static final Pattern TEAVM_TYPE = Pattern.compile("Lorg/teavm/classlib/([\\w/$]+);");
    static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    static final List<String> LIBS = List.of("com/google/", "it/unimi/", "org/slf4j/", "org/apache/",
            "io/netty/", "org/joml/", "org/jetbrains/", "org/lwjgl/", "org/objectweb/", "com/llamalad7/");

    public static void main(String[] args) throws IOException {
        if (args.length < 4) {
            throw new IllegalArgumentException("usage: <top500.tsv> <jar dir> <out dir> <provided jar|dir>...");
        }
        for (int i = 3; i < args.length; i++) {
            loadProvided(Path.of(args[i]));
        }
        List<Mod> mods = new ArrayList<>();
        List<String> lines = Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8);
        Map<String, Integer> col = new HashMap<>();
        String[] header = lines.get(0).split("\t");
        for (int i = 0; i < header.length; i++) {
            col.put(header[i], i);
        }
        for (String line : lines.subList(1, lines.size())) {
            String[] f = line.split("\t", -1);
            Mod mod = new Mod();
            mod.rank = f[col.get("rank")];
            mod.slug = f[col.get("slug")];
            mod.file = f[col.get("file")];
            try (InputStream in = Files.newInputStream(Path.of(args[1], mod.file))) {
                readJar(mod, in, true);
                classify(mod);
            } catch (IOException | RuntimeException e) {
                mod.error = e.toString();
            }
            mods.add(mod);
        }
        Path out = Path.of(args[2]);
        Files.createDirectories(out);
        writeTsv(mods, out.resolve("report.tsv"));
        writeMarkdown(mods, out.resolve("report.md"));
    }

    // ---- provided API -------------------------------------------------------------------

    static void loadProvided(Path path) throws IOException {
        if (Files.isDirectory(path)) {
            try (Stream<Path> files = Files.walk(path)) {
                for (Path p : files.filter(p -> p.toString().endsWith(".class")).toList()) {
                    addProvided(Files.readAllBytes(p));
                }
            }
        } else {
            try (ZipInputStream zip = new ZipInputStream(Files.newInputStream(path))) {
                for (ZipEntry e; (e = zip.getNextEntry()) != null; ) {
                    if (e.getName().endsWith(".class")) {
                        addProvided(zip.readAllBytes());
                    }
                }
            }
        }
    }

    static void addProvided(byte[] bytes) {
        ClassModel cm = ClassFile.of().parse(bytes);
        String name = fromTeaVm(cm.thisClass().asInternalName());
        if (name == null) {
            return; // a TeaVM helper class that stands for nothing in the JDK
        }
        Set<String> members = new HashSet<>();
        for (MethodModel m : cm.methods()) {
            members.add(m.methodName().stringValue() + mapDescriptor(m.methodType().stringValue()));
        }
        for (FieldModel f : cm.fields()) {
            members.add(f.fieldName().stringValue() + ":" + mapDescriptor(f.fieldType().stringValue()));
        }
        String superName = cm.superclass().map(c -> fromTeaVm(c.asInternalName())).orElse(null);
        List<String> interfaces = cm.interfaces().stream().map(c -> fromTeaVm(c.asInternalName())).toList();
        provided.merge(name, new Info(superName, interfaces, members), (a, b) -> {
            a.members().addAll(b.members());
            return a;
        });
    }

    /** org/teavm/classlib/java/util/THashMap -> java/util/HashMap; other names unchanged. */
    static String fromTeaVm(String internal) {
        String prefix = "org/teavm/classlib/";
        if (!internal.startsWith(prefix)) {
            return internal;
        }
        String rest = internal.substring(prefix.length());
        int slash = rest.lastIndexOf('/');
        String simple = rest.substring(slash + 1);
        return simple.startsWith("T") ? rest.substring(0, slash + 1) + simple.substring(1) : null;
    }

    static String mapDescriptor(String desc) {
        Matcher m = TEAVM_TYPE.matcher(desc);
        StringBuilder sb = new StringBuilder();
        while (m.find()) {
            String mapped = fromTeaVm("org/teavm/classlib/" + m.group(1));
            m.appendReplacement(sb, Matcher.quoteReplacement("L" + (mapped == null ? m.group(1) : mapped) + ";"));
        }
        return m.appendTail(sb).toString();
    }

    static boolean covered(Ref ref) {
        // TeaVM implements the invokedynamic bootstraps (lambdas, string concat, records) itself.
        if (ref.owner().startsWith("java/lang/invoke/") || ref.owner().startsWith("java/lang/runtime/")) {
            return true;
        }
        if (ref.member().isEmpty()) {
            return provided.containsKey(ref.owner());
        }
        List<String> queue = new ArrayList<>(List.of(ref.owner()));
        Set<String> seen = new HashSet<>();
        while (!queue.isEmpty()) {
            String owner = queue.removeLast();
            Info info = owner == null || !seen.add(owner) ? null : provided.get(owner);
            if (info == null) {
                continue;
            }
            if (info.members().contains(ref.member())) {
                return true;
            }
            queue.add(info.superName());
            queue.addAll(info.interfaces());
        }
        return false;
    }

    // ---- mod jars -----------------------------------------------------------------------

    static void readJar(Mod mod, InputStream in, boolean top) throws IOException {
        ZipInputStream zip = new ZipInputStream(in);
        for (ZipEntry e; (e = zip.getNextEntry()) != null; ) {
            String name = e.getName();
            if (name.endsWith(".class") && !name.startsWith("META-INF/versions/")) {
                readClass(mod, zip.readAllBytes());
            } else if (name.startsWith("META-INF/jars/") && name.endsWith(".jar")) {
                mod.nestedJars++;
                readJar(mod, new java.io.ByteArrayInputStream(zip.readAllBytes()), false);
            } else if (name.equals("fabric.mod.json")) {
                String json = new String(zip.readAllBytes(), StandardCharsets.UTF_8);
                mod.mixinConfig |= Pattern.compile("\"mixins\"\\s*:\\s*\\[\\s*[^\\]\\s]").matcher(json).find();
                if (top) {
                    Matcher ep = Pattern.compile("\"entrypoints\"\\s*:\\s*\\{(.*?)\\}\\s*[,}]", Pattern.DOTALL).matcher(json);
                    String eps = ep.find() ? ep.group(1) : "";
                    mod.mainEntrypoint = eps.contains("\"main\"");
                    mod.serverEntrypoint = eps.contains("\"server\"");
                    mod.clientEntrypoint = eps.contains("\"client\"");
                }
            }
        }
    }

    static void readClass(Mod mod, byte[] bytes) {
        ClassModel cm = ClassFile.of().parse(bytes);
        mod.classes++;
        mod.own.add(cm.thisClass().asInternalName());
        boolean mixin = false;
        for (PoolEntry entry : cm.constantPool()) {
            if (entry instanceof MemberRefEntry ref) {
                mod.refs.add(new Ref(ref.owner().asInternalName(),
                        ref.name().stringValue() + (ref.type().stringValue().startsWith("(")
                                ? ref.type().stringValue() : ":" + ref.type().stringValue())));
            } else if (entry instanceof ClassEntry c) {
                String name = c.asInternalName();
                int dims = 0;
                while (name.startsWith("[", dims)) {
                    dims++;
                }
                if (dims > 0) {
                    name = name.substring(dims);
                    if (!name.startsWith("L")) {
                        continue; // primitive array
                    }
                    name = name.substring(1, name.length() - 1);
                }
                mod.refs.add(new Ref(name, ""));
            } else if (entry instanceof Utf8Entry u && u.equalsString(MIXIN)) {
                mixin = true;
            }
        }
        if (mixin) {
            mod.mixinClasses++;
        }
    }

    static String category(String owner) {
        if (owner.startsWith("java/") || owner.startsWith("javax/") || owner.startsWith("jdk/") || owner.startsWith("sun/")) {
            return "jdk";
        }
        if (owner.startsWith("net/minecraft/") || owner.startsWith("com/mojang/")) {
            return "minecraft";
        }
        if (owner.startsWith("net/fabricmc/")) {
            return "fabric";
        }
        if (owner.startsWith("org/spongepowered/")) {
            return "mixin";
        }
        return LIBS.stream().anyMatch(owner::startsWith) ? "lib" : "other";
    }

    static void classify(Mod mod) {
        for (Ref ref : mod.refs) {
            if (mod.own.contains(ref.owner())) {
                continue;
            }
            String cat = category(ref.owner());
            mod.external.merge(cat, 1, Integer::sum);
            // Libraries are compiled from their real jars, not shimmed; other mods are dependencies.
            if (!cat.equals("lib") && !cat.equals("other") && !covered(ref)) {
                mod.missing.computeIfAbsent(cat, k -> new java.util.TreeSet<>()).add(ref.key());
            }
        }
    }

    static int missing(Mod mod, String cat) {
        return mod.missing.getOrDefault(cat, Set.of()).size();
    }

    static boolean shimTarget(Mod mod) {
        return mod.error == null && mod.classes > 0 && mod.mixinClasses == 0 && !mod.mixinConfig && mod.external.getOrDefault("other", 0) == 0;
    }

    // ---- output -------------------------------------------------------------------------

    static void writeTsv(List<Mod> mods, Path path) throws IOException {
        List<String> rows = new ArrayList<>();
        rows.add("rank\tslug\tclasses\tnested_jars\tmixin_classes\tentrypoints\tmissing_minecraft\tmissing_fabric"
                + "\tmissing_jdk\tmissing_mixin\tlib_refs\tother_mod_refs\terror");
        for (Mod m : mods) {
            String eps = (m.mainEntrypoint ? "main " : "") + (m.serverEntrypoint ? "server " : "") + (m.clientEntrypoint ? "client" : "");
            rows.add(String.join("\t", m.rank, m.slug, "" + m.classes, "" + m.nestedJars, "" + m.mixinClasses,
                    eps.trim(), "" + missing(m, "minecraft"), "" + missing(m, "fabric"), "" + missing(m, "jdk"),
                    "" + missing(m, "mixin"), "" + m.external.getOrDefault("lib", 0),
                    "" + m.external.getOrDefault("other", 0), m.error == null ? "" : m.error));
        }
        Files.write(path, rows, StandardCharsets.UTF_8);
    }

    static void writeMarkdown(List<Mod> mods, Path path) throws IOException {
        List<Mod> analysed = mods.stream().filter(m -> m.error == null).toList();
        long noCode = analysed.stream().filter(m -> m.classes == 0).count();
        List<Mod> ok = analysed.stream().filter(m -> m.classes > 0).toList();
        List<Mod> targets = ok.stream().filter(Analyze::shimTarget).toList();
        long mixinMods = ok.stream().filter(m -> m.mixinClasses > 0 || m.mixinConfig).count();
        long dependent = ok.stream().filter(m -> m.mixinClasses == 0 && !m.mixinConfig && m.external.getOrDefault("other", 0) > 0).count();
        long clientOnly = ok.stream().filter(m -> m.clientEntrypoint && !m.mainEntrypoint && !m.serverEntrypoint).count();
        long ready = targets.stream().filter(m -> m.missing.isEmpty()).count();

        // How many target mods reference each missing member (minecraft, fabric, jdk, mixin).
        Map<String, Integer> demand = new HashMap<>();
        for (Mod m : targets) {
            m.missing.values().forEach(keys -> keys.forEach(k -> demand.merge(k, 1, Integer::sum)));
        }
        List<Map.Entry<String, Integer>> ranked = new ArrayList<>(demand.entrySet());
        ranked.sort(Map.Entry.<String, Integer>comparingByValue().reversed().thenComparing(Map.Entry.comparingByKey()));

        // Coverage curve: implement members in demand order; a mod is unlocked once all its gaps are filled.
        Map<String, Integer> order = new HashMap<>();
        for (int i = 0; i < ranked.size(); i++) {
            order.put(ranked.get(i).getKey(), i + 1);
        }
        List<Integer> unlockAt = new ArrayList<>();
        for (Mod m : targets) {
            int need = 0;
            for (Set<String> keys : m.missing.values()) {
                for (String k : keys) {
                    need = Math.max(need, order.get(k));
                }
            }
            unlockAt.add(need);
        }
        unlockAt.sort(null);

        StringBuilder md = new StringBuilder();
        md.append("# Corpus report\n\n");
        md.append("Generated by `recompiler/corpus/Analyze.java` from `top500.tsv` (Modrinth, Fabric, 26.3, ")
                .append("server-capable, by downloads). Regenerate with `recompiler/corpus/report.ps1`.\n\n");
        md.append("| | mods |\n|---|---|\n");
        md.append("| analysed | ").append(analysed.size()).append(" of ").append(mods.size()).append(" |\n");
        md.append("| no Java code (data/resource packs as mods; need datapack support, not the recompiler) | ")
                .append(noCode).append(" |\n");
        md.append("| with code | ").append(ok.size()).append(" |\n");
        md.append("| use Mixin (need bytecode patching of Minecraft, not a shim) | ").append(mixinMods).append(" |\n");
        md.append("| no Mixin, but call into another mod | ").append(dependent).append(" |\n");
        md.append("| no Mixin, self-contained: the shim's target | ").append(targets.size()).append(" |\n");
        md.append("| ... of which fully covered today | ").append(ready).append(" |\n");
        md.append("| client entrypoint only | ").append(clientOnly).append(" |\n\n");

        md.append("## Coverage curve (shim targets)\n\n");
        md.append("Members (Minecraft, Fabric, JDK-in-TeaVM, Mixin runtime) still missing, implemented in demand order.\n\n");
        md.append("| implement this many members | mods fully covered |\n|---|---|\n");
        for (int n : new int[] {0, 25, 50, 100, 250, 500, 1000, 2500, 5000}) {
            if (n > ranked.size() && n != 0) {
                break;
            }
            long count = unlockAt.stream().filter(u -> u <= n).count();
            md.append("| ").append(n).append(" | ").append(count).append(" (").append(pct(count, targets.size())).append(") |\n");
        }
        md.append("| ").append(ranked.size()).append(" (all) | ").append(targets.size()).append(" |\n\n");

        md.append("## Most-needed missing members (top 60, by number of shim-target mods)\n\n");
        md.append("| mods | member |\n|---|---|\n");
        ranked.stream().limit(60).forEach(e -> md.append("| ").append(e.getValue()).append(" | `").append(e.getKey()).append("` |\n"));

        md.append("\n## Smallest gaps (next fixtures)\n\n| rank | mod | missing members |\n|---|---|---|\n");
        targets.stream().filter(m -> !m.missing.isEmpty())
                .sorted((a, b) -> Integer.compare(total(a), total(b))).limit(20)
                .forEach(m -> md.append("| ").append(m.rank).append(" | ").append(m.slug).append(" | ").append(total(m)).append(" |\n"));

        List<Mod> failed = mods.stream().filter(m -> m.error != null).toList();
        if (!failed.isEmpty()) {
            md.append("\n## Not analysed\n\n");
            failed.forEach(m -> md.append("- ").append(m.slug).append(": ").append(m.error).append('\n'));
        }
        Files.writeString(path, md.toString(), StandardCharsets.UTF_8);
    }

    static int total(Mod m) {
        return m.missing.values().stream().mapToInt(Set::size).sum();
    }

    static String pct(long n, long of) {
        return of == 0 ? "0%" : Math.round(100.0 * n / of) + "%";
    }
}

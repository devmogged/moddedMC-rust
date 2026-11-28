package dev.moddedmc.companion.rig;

import com.geckolib.animatable.GeoAnimatable;
import com.geckolib.animatable.instance.AnimatableInstanceCache;
import com.geckolib.animatable.manager.AnimatableManager;
import com.geckolib.animation.AnimationController;
import com.geckolib.animation.RawAnimation;
import com.geckolib.cache.BakedAnimationCache;
import com.geckolib.cache.BakedModelCache;
import com.geckolib.cache.GeckoLibResources;
import com.geckolib.constant.DataTickets;
import com.geckolib.constant.dataticket.DataTicket;
import com.geckolib.model.GeoModel;
import com.geckolib.renderer.GeoObjectRenderer;
import com.geckolib.renderer.base.GeoRenderState;
import com.geckolib.renderer.base.RenderPassInfo;
import com.geckolib.util.GeckoLibUtil;
import com.geckolib.util.RenderUtil;
import com.google.gson.JsonParser;
import com.mojang.math.Axis;
import com.mojang.serialization.Codec;
import com.mojang.serialization.JsonOps;
import com.mojang.serialization.codecs.RecordCodecBuilder;
import java.io.IOException;
import java.lang.ref.WeakReference;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.WeakHashMap;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.OrderedSubmitNodeCollector;
import net.minecraft.client.renderer.rendertype.RenderType;
import net.minecraft.core.component.DataComponents;
import net.minecraft.resources.FileToIdConverter;
import net.minecraft.resources.Identifier;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.item.component.CustomData;
import net.minecraft.world.phys.Vec3;
import org.joml.Vector3f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class DisplayRigs {
    private static final Logger LOGGER = LoggerFactory.getLogger("moddedmc-companion");
    private static final FileToIdConverter FILES = FileToIdConverter.json("moddedmc/rigs");
    private static final DataTicket<Identifier> TEXTURE = DataTicket.create("moddedmc_rig_texture", Identifier.class);
    private static final DataTicket<Identifier> GEOMETRY = DataTicket.create("moddedmc_rig_geometry", Identifier.class);
    private static final DataTicket<Boolean> PREVIEW = DataTicket.create("moddedmc_rig_preview", Boolean.class);
    private static final DataTicket<SeatFrame> SEATS = DataTicket.create("moddedmc_rig_seats", SeatFrame.class);
    private static final RigFrame HIDDEN = (pose, collector, camera) -> {};
    private static final Map<ItemDisplay, RigInstance> INSTANCES = new WeakHashMap<>();
    private static BakedModelCache modelCache;
    private static BakedAnimationCache animationCache;
    private static Map<Identifier, RigRenderer> rigs = Map.of();
    private static Set<Identifier> hiddenParts = Set.of();

    private DisplayRigs() {}

    public static RigFrame extract(ItemDisplay display, int light, float delta) {
        var itemState = display.itemRenderState();
        if (itemState == null) return null;
        var stack = itemState.itemStack();
        var itemModel = stack.get(DataComponents.ITEM_MODEL);
        if (itemModel == null) return null;

        refresh();

        RigRenderer renderer = rigs.get(itemModel);
        if (renderer == null) return hiddenParts.contains(itemModel) ? HIDDEN : null;
        RigInstance rig = INSTANCES.get(display);
        if (rig == null || rig.owner.get() != display || rig.renderer != renderer) {
            // Entity equality uses the network ID, which a server can reuse.
            INSTANCES.remove(display);
            rig = new RigInstance(display, renderer, false, false);
            INSTANCES.put(display, rig);
        }
        rig.selectAppearance(stack.get(DataComponents.CUSTOM_DATA));

        var state = renderer.createRenderState(rig, display);
        state.addGeckolibData(DataTickets.PACKED_LIGHT, light);
        renderer.fillRenderState(rig, display, state, delta);
        return (pose, collector, camera) -> renderer.performRenderPass(state, pose, collector, camera);
    }

    private static void refresh() {
        var models = GeckoLibResources.getBakedModels();
        var animations = GeckoLibResources.getBakedAnimations();
        if (modelCache != models || animationCache != animations) {
            modelCache = models;
            animationCache = animations;
            rigs = load(Minecraft.getInstance().getResourceManager(), models, animations);
            INSTANCES.clear();
            RiderSeats.clear();
        }

    }

    /** A screen owns its preview animation cache; never touches a world's animation instance or seats. */
    public static final class Preview {
        private RigInstance rig;
        private String entityId;

        public RigFrame extract(Identifier model, String texture, String id, float delta) {
            refresh();
            RigRenderer renderer = rigs.get(model);
            if (renderer == null) return null;
            ItemDisplay live = findDisplay(model, id);
            boolean still = live == null;
            if (rig == null || rig.renderer != renderer || rig.still != still || !id.equals(entityId)
                    || rig.owner.get() != live) {
                rig = new RigInstance(live, renderer, true, still);
                entityId = id;
            }
            if (live != null) rig.selectAppearance(live.itemRenderState().itemStack().get(DataComponents.CUSTOM_DATA));
            else rig.texture = renderer.textures.getOrDefault(texture, renderer.definition.texture());
            var state = renderer.createRenderState(rig, live);
            state.addGeckolibData(DataTickets.PACKED_LIGHT, 0xf000f0);
            renderer.fillRenderState(rig, live, state, delta);
            return (pose, collector, camera) -> renderer.performRenderPass(state, pose, collector, camera);
        }

        private static ItemDisplay findDisplay(Identifier model, String id) {
            var level = Minecraft.getInstance().level;
            if (level == null || id.isEmpty()) return null;
            for (Entity entity : level.entitiesForRendering()) {
                if (!(entity instanceof ItemDisplay display) || display.itemRenderState() == null) continue;
                var stack = display.itemRenderState().itemStack();
                if (!model.equals(stack.get(DataComponents.ITEM_MODEL))) continue;
                var data = stack.get(DataComponents.CUSTOM_DATA);
                if (data == null) continue;
                var mount = level.getEntity(data.copyTag().getIntOr("moddedmc:rig_mount", -1));
                if (mount != null && mount.getUUID().toString().replace("-", "").equals(id)) return display;
            }
            return null;
        }
    }

    private static Map<Identifier, RigRenderer> load(
            ResourceManager resources, BakedModelCache models, BakedAnimationCache animations) {
        var loaded = new HashMap<Identifier, RigRenderer>();
        var hidden = new HashSet<Identifier>();
        FILES.listMatchingResources(resources).forEach((file, resource) -> {
            try (var reader = resource.openAsReader()) {
                var definition = Definition.CODEC.parse(JsonOps.INSTANCE, JsonParser.parseReader(reader)).getOrThrow();
                var clips = animations.cache().get(definition.animations());
                if (!models.cache().containsKey(definition.geometry())
                        || clips == null || !clips.animations().containsKey(definition.clip())
                        || resources.getResource(definition.texture()).isEmpty()) {
                    LOGGER.warn("Ignoring rig {} with missing geometry, clip, or texture", file);
                    return;
                }
                var playback = new HashMap<String, RawAnimation>();
                clips.animations().keySet().forEach(name -> playback.put(name, RawAnimation.begin().thenPlay(name)));
                var textures = new HashMap<String, Identifier>();
                definition.textures().forEach((name, id) -> {
                    if (resources.getResource(id).isPresent()) textures.put(name, id);
                });
                loaded.put(FILES.fileToId(file), new RigRenderer(definition, Map.copyOf(playback), Map.copyOf(textures)));
                hidden.addAll(definition.hideParts());
            } catch (IOException | RuntimeException error) {
                LOGGER.warn("Ignoring invalid rig {}: {}", file, error.getMessage());
            }
        });
        hiddenParts = Set.copyOf(hidden);
        return loaded;
    }

    private record Definition(Identifier geometry, Identifier animations, Identifier texture, String clip,
                              float scale, float yOffset, float yawOffset, List<Identifier> hideParts,
                              Map<String, Identifier> textures, List<Seat> riderSeats, java.util.Optional<Identifier> previewGeometry) {
        private static final Codec<Definition> CODEC = RecordCodecBuilder.create(instance -> instance.group(
                Identifier.CODEC.fieldOf("geometry").forGetter(Definition::geometry),
                Identifier.CODEC.fieldOf("animations").forGetter(Definition::animations),
                Identifier.CODEC.fieldOf("texture").forGetter(Definition::texture),
                Codec.STRING.fieldOf("clip").forGetter(Definition::clip),
                Codec.floatRange(0.01f, 16f).optionalFieldOf("scale", 1f).forGetter(Definition::scale),
                Codec.floatRange(-8f, 8f).optionalFieldOf("y_offset", 0f).forGetter(Definition::yOffset),
                Codec.floatRange(-180f, 180f).optionalFieldOf("yaw_offset", 0f).forGetter(Definition::yawOffset),
                Identifier.CODEC.listOf().optionalFieldOf("hide_parts", List.of()).forGetter(Definition::hideParts),
                Codec.unboundedMap(Codec.STRING, Identifier.CODEC).optionalFieldOf("textures", Map.of()).forGetter(Definition::textures),
                Seat.CODEC.listOf().optionalFieldOf("rider_seats", List.of()).forGetter(Definition::riderSeats),
                Identifier.CODEC.optionalFieldOf("preview_geometry").forGetter(Definition::previewGeometry)
        ).apply(instance, Definition::new));
    }

    private record Seat(String bone, float x, float y, float z, float height, int maxAge) {
        private static final Codec<Seat> CODEC = RecordCodecBuilder.create(instance -> instance.group(
                Codec.STRING.fieldOf("bone").forGetter(Seat::bone),
                Codec.FLOAT.optionalFieldOf("x", 0f).forGetter(Seat::x),
                Codec.FLOAT.optionalFieldOf("y", 0f).forGetter(Seat::y),
                Codec.FLOAT.optionalFieldOf("z", 0f).forGetter(Seat::z),
                Codec.FLOAT.optionalFieldOf("height", 0f).forGetter(Seat::height),
                Codec.intRange(0, Integer.MAX_VALUE).optionalFieldOf("max_age", Integer.MAX_VALUE).forGetter(Seat::maxAge)
        ).apply(instance, Seat::new));
    }

    private record SeatFrame(ItemDisplay display, Entity mount, float delta) {}

    private static final class RigInstance implements GeoAnimatable {
        private final AnimatableInstanceCache cache = GeckoLibUtil.createInstanceCache(this, false);
        private final WeakReference<ItemDisplay> owner;
        private final RigRenderer renderer;
        private final boolean preview, still;
        private RawAnimation clip;
        private Identifier texture;
        private CustomData appearanceData;
        private int mountId = -1;

        private RigInstance(ItemDisplay display, RigRenderer renderer, boolean preview, boolean still) {
            this.owner = new WeakReference<>(display);
            this.renderer = renderer;
            this.preview = preview;
            this.still = still;
            this.clip = renderer.defaultClip;
            this.texture = renderer.definition.texture();
        }

        private void selectAppearance(CustomData data) {
            if (appearanceData == data) return;
            appearanceData = data;
            var tag = data == null ? null : data.copyTag();
            mountId = tag == null ? -1 : tag.getIntOr("moddedmc:rig_mount", -1);
            clip = tag == null ? renderer.defaultClip : renderer.clips.getOrDefault(
                    tag.getStringOr("moddedmc:rig_clip", ""), renderer.defaultClip);
            texture = tag == null ? renderer.definition.texture() : renderer.textures.getOrDefault(
                    tag.getStringOr("moddedmc:rig_texture", ""), renderer.definition.texture());
        }

        @Override
        public void registerControllers(AnimatableManager.ControllerRegistrar registrar) {
            if (!still) registrar.add(new AnimationController<RigInstance>("display", 3, test -> test.setAndContinue(clip)));
        }

        @Override
        public AnimatableInstanceCache getAnimatableInstanceCache() {
            return cache;
        }
    }

    private static final class RigRenderer extends GeoObjectRenderer<RigInstance, ItemDisplay, GeoRenderState> {
        private final Definition definition;
        private final Map<String, RawAnimation> clips;
        private final RawAnimation defaultClip;
        private final Map<String, Identifier> textures;

        private RigRenderer(Definition definition, Map<String, RawAnimation> clips, Map<String, Identifier> textures) {
            super(new GeoModel<>() {
                @Override
                public Identifier getModelResource(GeoRenderState state) {
                    return state.getOrDefaultGeckolibData(GEOMETRY, definition.geometry());
                }

                @Override
                public Identifier getTextureResource(GeoRenderState state) {
                    return state.getOrDefaultGeckolibData(TEXTURE, definition.texture());
                }

                @Override
                public Identifier getAnimationResource(RigInstance rig) {
                    return definition.animations();
                }
            });
            this.definition = definition;
            this.clips = clips;
            this.textures = textures;
            this.defaultClip = clips.get(definition.clip());
        }

        @Override
        public void addRenderData(RigInstance rig, ItemDisplay display, GeoRenderState state, float delta) {
            state.addGeckolibData(DataTickets.TICK, display == null ? 0d : display.tickCount + (double) delta);
            state.addGeckolibData(TEXTURE, rig.texture);
            state.addGeckolibData(PREVIEW, rig.preview);
            if (rig.still) definition.previewGeometry().filter(modelCache.cache()::containsKey)
                    .ifPresent(geometry -> state.addGeckolibData(GEOMETRY, geometry));
            if (!rig.preview && !definition.riderSeats().isEmpty() && rig.mountId >= 0
                    && Minecraft.getInstance().level != null) {
                Entity mount = Minecraft.getInstance().level.getEntity(rig.mountId);
                if (mount != null && mount.isVehicle()) {
                    state.addGeckolibData(SEATS, new SeatFrame(display, mount, delta));
                }
            }
        }

        @Override
        public void submitRenderTasks(RenderPassInfo<GeoRenderState> pass,
                                      OrderedSubmitNodeCollector collector, RenderType renderType) {
            SeatFrame frame = pass.getGeckolibData(SEATS);
            if (frame != null) {
                // The pose already includes the interpolated item-display transform and rig
                // adjustments. Evaluate the same GeckoLib snapshots as the geometry submission.
                pass.renderPosed(() -> {
                    var seats = definition.riderSeats();
                    var positions = new RiderSeats.Anchor[seats.size()];
                    for (int i = 0; i < seats.size(); i++) {
                        Seat seat = seats.get(i);
                        var bone = pass.model().getBone(seat.bone()).orElse(null);
                        if (bone == null) continue;
                        var pose = pass.poseStack();
                        pose.pushPose();
                        try {
                            RenderUtil.transformToBone(pose, bone);
                            var point = pose.last().pose().transformPosition(
                                    new Vector3f(seat.x() / 16f, seat.y() / 16f, seat.z() / 16f));
                            Vec3 world = pass.cameraState().pos.add(point.x, point.y + seat.height(), point.z);
                            positions[i] = RiderSeats.anchor(frame.mount(), frame.delta(), world, seat.maxAge());
                        } finally {
                            pose.popPose();
                        }
                    }
                    RiderSeats.sample(frame.display(), frame.mount(), positions);
                });
            }
            super.submitRenderTasks(pass, collector, renderType);
        }

        @Override
        public void adjustRenderPose(RenderPassInfo<GeoRenderState> pass) {
            if (Boolean.TRUE.equals(pass.getGeckolibData(PREVIEW))) return;
            var pose = pass.poseStack();
            pose.translate(0, definition.yOffset(), 0);
            if (definition.yawOffset() != 0) pose.rotate(Axis.YP.rotationDegrees(definition.yawOffset()));
            float scale = definition.scale();
            pose.scale(scale, scale, scale);
        }
    }
}

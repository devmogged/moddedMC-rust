package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.Companion;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(BlockBehaviour.BlockStateBase.class)
abstract class ScaffoldingShapeMixin {
    @Inject(method = {
            "getCollisionShape(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;"
                    + "Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;",
            "getVisualShape"},
            at = @At("HEAD"), cancellable = true)
    private void moddedmc$collision(BlockGetter level, BlockPos pos, CollisionContext context,
            CallbackInfoReturnable<VoxelShape> result) {
        var scaffold = Companion.scaffold((BlockState)(Object)this);
        if (scaffold != null) result.setReturnValue(scaffold.collision(pos, context));
    }

    @Inject(method = {
            "getCollisionShape(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)"
                    + "Lnet/minecraft/world/phys/shapes/VoxelShape;", "getBlockSupportShape"},
            at = @At("HEAD"), cancellable = true)
    private void moddedmc$staticShape(BlockGetter level, BlockPos pos, CallbackInfoReturnable<VoxelShape> result) {
        var scaffold = Companion.scaffold((BlockState)(Object)this);
        if (scaffold != null) result.setReturnValue(scaffold.collision(pos, CollisionContext.empty()));
    }

    @Inject(method = "getShape(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;"
            + "Lnet/minecraft/world/phys/shapes/CollisionContext;)Lnet/minecraft/world/phys/shapes/VoxelShape;",
            at = @At("HEAD"), cancellable = true)
    private void moddedmc$outline(BlockGetter level, BlockPos pos, CollisionContext context,
            CallbackInfoReturnable<VoxelShape> result) {
        var scaffold = Companion.scaffold((BlockState)(Object)this);
        if (scaffold != null) result.setReturnValue(scaffold.outline(context));
    }

    @Inject(method = "isFaceSturdy(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;"
            + "Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/SupportType;)Z",
            at = @At("HEAD"), cancellable = true)
    private void moddedmc$support(BlockGetter level, BlockPos pos, Direction face, SupportType type,
            CallbackInfoReturnable<Boolean> result) {
        var state = (BlockState)(Object)this;
        if (Companion.scaffold(state) != null) result.setReturnValue(type.isSupporting(state, level, pos, face));
    }

    @Inject(method = "getInteractionShape", at = @At("HEAD"), cancellable = true)
    private void moddedmc$interaction(BlockGetter level, BlockPos pos, CallbackInfoReturnable<VoxelShape> result) {
        if (Companion.scaffold((BlockState)(Object)this) != null) result.setReturnValue(Shapes.block());
    }

    @Inject(method = "skipRendering", at = @At("HEAD"), cancellable = true)
    private void moddedmc$hideFace(BlockState neighbor, Direction direction, CallbackInfoReturnable<Boolean> result) {
        if (direction.getAxis() == Direction.Axis.Y) return;
        var scaffold = Companion.scaffold((BlockState)(Object)this);
        if (scaffold == null) return;
        var other = Companion.scaffold(neighbor);
        if (other != null && (scaffold.bottom() || !other.bottom())) result.setReturnValue(true);
    }
}

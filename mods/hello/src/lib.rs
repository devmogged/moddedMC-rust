//! M0 smoke test: proves a WASM component loads on the pinned Pumpkin build.
use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Server,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    register_plugin,
    text::TextComponent,
};

struct Hello;

struct HelloCmd;

impl CommandHandler for HelloCmd {
    fn handle(&self, sender: CommandSender, _: Server, _: ConsumedArgs) -> Result<i32, CommandError> {
        sender.send_message(TextComponent::text("hello from a moddedMC-rust WASM mod"));
        Ok(1)
    }
}

impl Plugin for Hello {
    fn new() -> Self {
        Hello
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "hello".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["moddedMC-rust".into()],
            description: "M0 WASM load smoke test".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let cmd = Command::new(&["hello".into()], "Say hello").execute(HelloCmd);
        context.register_command(cmd, "minecraft:command.help");
        Ok(())
    }
}

register_plugin!(Hello);

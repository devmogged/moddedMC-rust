import { Binary } from "lucide-react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "~/components/ui/empty";

export function ComingSoonTab() {
  return (
    <Empty className="h-full">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <Binary />
        </EmptyMedia>
        <EmptyTitle>Jar → Rust is coming</EmptyTitle>
        <EmptyDescription>
          Turn a Fabric mod's .jar into a native moddedMC mod. It isn't ready yet.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}

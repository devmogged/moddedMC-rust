package net.fabricmc.fabric.api.event;

import java.util.ArrayList;
import java.util.List;

/** Listener list. The shim runtime fires events itself, so there is no invoker factory. */
public class Event<T> {
    private final List<T> listeners = new ArrayList<>();

    public void register(T listener) {
        listeners.add(listener);
    }

    public List<T> listeners() {
        return listeners;
    }
}

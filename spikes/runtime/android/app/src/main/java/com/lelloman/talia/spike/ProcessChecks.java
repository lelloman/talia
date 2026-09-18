package com.lelloman.talia.spike;

import android.content.*;
import android.os.*;
import org.json.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.IntSupplier;

// Parent-owned generation/resources and Binder lifecycle test harness.
final class ProcessChecks implements AutoCloseable {
    private final Context context;
    private final Handler main;
    private final IntSupplier ticks;
    private final List<Slot> slots = new ArrayList<>();
    private long generation = 0;
    private boolean closed;
    ProcessChecks(Context context, Handler main, IntSupplier ticks) {
        this.context=context; this.main=main; this.ticks=ticks;
    }
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
    private <T> T onMain(Callable<T> task) throws Exception {
        if (Looper.myLooper() == main.getLooper()) return task.call();
        CompletableFuture<T> future = new CompletableFuture<>();
        main.post(() -> { try { future.complete(task.call()); } catch (Throwable e) { future.completeExceptionally(e); } });
        return future.get(10, TimeUnit.SECONDS);
    }
    private static final class Pending {
        final CompletableFuture<Integer> future = new CompletableFuture<>();
        Runnable timeout;
    }
    private final class Slot implements ServiceConnection {
        final long generation = ++ProcessChecks.this.generation;
        final CompletableFuture<Void> connected = new CompletableFuture<>();
        final CompletableFuture<Void> died = new CompletableFuture<>();
        final CompletableFuture<Void> hanging = new CompletableFuture<>();
        final Map<Long, Pending> pending = new HashMap<>();
        final Messenger replies = new Messenger(new Handler(main.getLooper(), message -> { receive(message); return true; }));
        final IBinder.DeathRecipient death = () -> main.post(() -> {
            binderDeath=true; died.complete(null); retire("Binder death", false);
        });
        Messenger remote;
        IBinder binder;
        long nextToken;
        volatile int pid;
        volatile boolean active=true, bound=false, subscribed=false, stalled=false, binderDeath=false, watchdog=false;
        Slot(Class<?> service) {
            if (closed) throw new IllegalStateException("harness closed");
            slots.add(this);
            bound=context.bindService(new Intent(context, service), this, Context.BIND_AUTO_CREATE);
            if (!bound) { active=false; connected.completeExceptionally(new IllegalStateException("bind failed")); }
        }
        @Override public void onServiceConnected(ComponentName name, IBinder binder) {
            if (!active) return;
            if (remote != null) { retire("unexpected automatic reconnect", true); return; }
            this.binder=binder; remote=new Messenger(binder);
            try { binder.linkToDeath(death, 0); connected.complete(null); }
            catch (RemoteException e) { retire("dead binder on connect", false); }
        }
        @Override public void onServiceDisconnected(ComponentName name) { retire("service disconnected", false); }
        @Override public void onBindingDied(ComponentName name) { retire("binding died", false); }
        @Override public void onNullBinding(ComponentName name) { retire("null binding", false); }
        CompletableFuture<Integer> command(int action, long timeoutMs) {
            Pending call = new Pending();
            if (!active || remote == null) { call.future.completeExceptionally(new IllegalStateException("retired generation")); return call.future; }
            check(pending.size() < 16, "parent test queue exhausted");
            long token = ++nextToken;
            call.timeout = () -> { watchdog=true; retire("watchdog", true); };
            pending.put(token, call); main.postDelayed(call.timeout, timeoutMs);
            Message request=Message.obtain(null, action); request.replyTo=replies;
            Bundle data=new Bundle(); data.putLong("token", token); request.setData(data);
            try { remote.send(request); } catch (RemoteException e) { retire("send failed", false); }
            return call.future;
        }
        void receive(Message message) {
            if (!active) return;
            Bundle data=message.getData();
            Pending call=pending.get(data.getLong("token"));
            if (call == null) return;
            int senderPid=data.getInt("pid");
            if (pid != 0 && pid != senderPid) { retire("PID changed within generation", true); return; }
            pid=senderPid;
            if (message.what == RuntimeService.STARTED) { hanging.complete(null); return; }
            pending.remove(data.getLong("token")); main.removeCallbacks(call.timeout);
            int value=data.getInt("value");
            if (value < 0) call.future.completeExceptionally(new IllegalStateException("native check failed"));
            else call.future.complete(value);
        }
        void retire(String reason, boolean kill) {
            if (!active) return;
            active=false; subscribed=false; stalled=false;
            connected.completeExceptionally(new IllegalStateException(reason));
            for (Pending call:pending.values()) {
                main.removeCallbacks(call.timeout); call.future.completeExceptionally(new IllegalStateException(reason));
            }
            pending.clear();
            if (kill && pid != 0 && pid != android.os.Process.myPid()) android.os.Process.killProcess(pid);
            if (bound) { context.unbindService(this); bound=false; }
        }
        void close() {
            // Explicit teardown also detaches death notification registrations.
            if (binder != null) { try { binder.unlinkToDeath(death,0); } catch (NoSuchElementException ignored) { } }
            retire("test closed", true);
        }
    }
    private Slot start(Class<?> service) throws Exception {
        Slot slot=onMain(() -> new Slot(service));
        slot.connected.get(10, TimeUnit.SECONDS);
        check(call(slot, RuntimeService.INIT) == 1, "guest did not request subscription");
        onMain(() -> { slot.subscribed=true; return null; });
        check(call(slot, RuntimeService.GRANT) == 2, "guest did not request stalled call");
        onMain(() -> { slot.stalled=true; return null; });
        check(slot.pid != android.os.Process.myPid(), "runtime is in UI process");
        return slot;
    }
    private int call(Slot slot, int action) throws Exception {
        return onMain(() -> slot.command(action, 5000)).get(8, TimeUnit.SECONDS);
    }
    private boolean staleReply(Slot destination, long originGeneration, long token) throws Exception {
        return onMain(() -> {
            // Origin is captured by the host route, never read from guest payloads.
            if (!destination.active || destination.generation != originGeneration) return false;
            Message reply=Message.obtain(null,RuntimeService.RESULT);
            Bundle data=new Bundle(); data.putLong("token",token); data.putInt("pid",destination.pid); data.putInt("value",999);
            reply.setData(data); destination.receive(reply); return true;
        });
    }
    private boolean event(Slot destination, long originGeneration) throws Exception {
        CompletableFuture<Integer> result=onMain(() -> {
            if (!destination.active || destination.generation != originGeneration) return null;
            return destination.command(RuntimeService.EVENT,5000);
        });
        if (result == null) return false;
        result.get(8,TimeUnit.SECONDS); return true;
    }
    JSONObject run() throws Exception {
        int startTicks=onMain(() -> ticks.getAsInt());
        long started=SystemClock.elapsedRealtime();
        Slot survivor=start(RuntimeService.B.class);
        JSONArray faults=new JSONArray(); int events=0;
        for (int action:new int[]{RuntimeService.ABORT,RuntimeService.HANG}) {
            Slot victim=start(RuntimeService.A.class);
            check(victim.pid != survivor.pid, "runtimes share a process");
            check(call(victim,RuntimeService.DIRTY)==9,"dirty VM failed");
            int oldPid=victim.pid;
            CompletableFuture<Integer> failure=onMain(() -> victim.command(action, action==RuntimeService.HANG ? 1500 : 5000));
            if (action==RuntimeService.HANG) victim.hanging.get(5,TimeUnit.SECONDS);
            check(call(survivor,RuntimeService.PING)==2,"survivor stopped during fault");
            check(call(survivor,RuntimeService.EVENT)==++events,"survivor event lost");
            if (action==RuntimeService.HANG) check(victim.active,"watchdog fired before concurrent progress");
            boolean rejected=false;
            try { failure.get(8,TimeUnit.SECONDS); } catch (ExecutionException expected) { rejected=true; }
            check(rejected,"fault command unexpectedly succeeded");
            victim.died.get(8,TimeUnit.SECONDS);
            check(victim.binderDeath,"Binder death was not observed");
            check(action==RuntimeService.HANG ? victim.watchdog : !victim.watchdog,"wrong retirement path");
            check(onMain(() -> !victim.active && !victim.bound && !victim.subscribed && !victim.stalled && victim.pending.isEmpty()),"retired resources leaked");
            check(survivor.active && survivor.subscribed && survivor.stalled,"survivor resources lost");
            Slot replacement=start(RuntimeService.A.class);
            check(replacement.pid != oldPid,"replacement did not get a fresh process");
            check(call(replacement,RuntimeService.BASELINE)==1,"replacement retained dirty state");
            CompletableFuture<Integer> recovered=onMain(() -> {
                CompletableFuture<Integer> pending=replacement.command(RuntimeService.SUITE,5000);
                check(replacement.nextToken==victim.nextToken,"request IDs did not collide");
                check(replacement.pending.containsKey(victim.nextToken),"replacement has no colliding pending command");
                check(!staleReply(replacement,victim.generation,victim.nextToken),"old generation reply accepted");
                check(!pending.isDone(),"stale response resolved replacement command");
                return pending;
            });
            check(recovered.get(8,TimeUnit.SECONDS)==20,"replacement native suite failed");
            check(!event(replacement,victim.generation),"old generation event accepted");
            check(call(replacement,RuntimeService.BASELINE)==1,"stale delivery changed replacement");
            check(call(survivor,RuntimeService.PING)==2,"survivor failed after rebind");
            JSONObject result=new JSONObject();
            result.put("fault",action==RuntimeService.ABORT ? "native_abort" : "native_hang");
            result.put("old_pid",oldPid); result.put("replacement_pid",replacement.pid);
            result.put("binder_death",true); result.put("pending_command_rejected",true);
            result.put("resources_retired",true); result.put("survivor_progress",true);
            result.put("fresh_process",true); result.put("baseline_restored",true);
            result.put("stale_generation_rejected",true); result.put("colliding_request_ids",true); result.put("replacement_full_suite",true);
            result.put("watchdog",victim.watchdog); faults.put(result);
            onMain(() -> { replacement.retire("test teardown",true); victim.close(); return null; });
            // Wait for explicit teardown before reusing the same service component.
            replacement.died.get(8,TimeUnit.SECONDS);
            onMain(() -> { replacement.close(); return null; });
        }
        int uiTicks=onMain(() -> ticks.getAsInt())-startTicks;
        check(uiTicks>0,"UI heartbeat stopped");
        onMain(() -> { survivor.close(); return null; });
        boolean empty=onMain(() -> slots.stream().allMatch(s -> !s.active && !s.bound && !s.subscribed && !s.stalled && s.pending.isEmpty()));
        check(empty,"final resources leaked");
        return new JSONObject().put("passed",true).put("host","android-service-process")
            .put("ui_pid",android.os.Process.myPid()).put("survivor_pid",survivor.pid)
            .put("faults",faults).put("ui_ticks",uiTicks).put("resources_empty",empty)
            .put("elapsed_ms",SystemClock.elapsedRealtime()-started);
    }
    @Override public void close() {
        try { onMain(() -> { closed=true; for (Slot slot:slots) slot.close(); return null; }); }
        catch (Exception e) { throw new IllegalStateException("service cleanup failed",e); }
    }
}

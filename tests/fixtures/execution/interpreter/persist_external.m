function y = persist_external(seed)
persistent p;
if seed > 0
    p = seed;
end
y = p;
end

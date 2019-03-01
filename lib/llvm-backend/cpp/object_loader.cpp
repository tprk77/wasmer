#include "object_loader.hh"
#include <iostream>
#include <memory>

struct MemoryManager : llvm::RuntimeDyld::MemoryManager {
public:
    MemoryManager(callbacks_t callbacks) : callbacks(callbacks) {}

    virtual ~MemoryManager() override {
        // Deallocate all of the allocated memory.
        callbacks.dealloc_memory(code_section.base, code_section.size);
        callbacks.dealloc_memory(read_section.base, read_section.size);
        callbacks.dealloc_memory(readwrite_section.base, readwrite_section.size);
    }

    virtual uint8_t* allocateCodeSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name) override {
        return allocate_bump(code_section, code_bump_ptr, size, alignment);
    }

    virtual uint8_t* allocateDataSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name, bool read_only) override {
        // Allocate from the read-only section or the read-write section, depending on if this allocation
        // should be read-only or not.
        if (read_only) {
            return allocate_bump(read_section, read_bump_ptr, size, alignment);
        } else {
            return allocate_bump(readwrite_section, readwrite_bump_ptr, size, alignment);
        }
    }

    virtual void reserveAllocationSpace(
        uintptr_t code_size,
        uint32_t code_align,
        uintptr_t read_data_size,
        uint32_t read_data_align,
        uintptr_t read_write_data_size,
        uint32_t read_write_data_align
    ) override {
        uint8_t *code_ptr_out = nullptr;
        size_t code_size_out = 0;
        auto code_result = callbacks.alloc_memory(code_size, PROTECT_READ_WRITE, &code_ptr_out, &code_size_out);
        assert(code_result == RESULT_OK);
        code_section = Section { code_ptr_out, code_size_out };
        code_bump_ptr = (uintptr_t)code_ptr_out;

        uint8_t *read_ptr_out = nullptr;
        size_t read_size_out = 0;
        auto read_result = callbacks.alloc_memory(read_data_size, PROTECT_READ_WRITE, &read_ptr_out, &read_size_out);
        assert(read_result == RESULT_OK);
        read_section = Section { read_ptr_out, read_size_out };
        read_bump_ptr = (uintptr_t)read_ptr_out;

        uint8_t *readwrite_ptr_out = nullptr;
        size_t readwrite_size_out = 0;
        auto readwrite_result = callbacks.alloc_memory(read_write_data_size, PROTECT_READ_WRITE, &readwrite_ptr_out, &readwrite_size_out);
        assert(readwrite_result == RESULT_OK);
        readwrite_section = Section { readwrite_ptr_out, readwrite_size_out };
        readwrite_bump_ptr = (uintptr_t)readwrite_ptr_out;
    }

    /* Turn on the `reserveAllocationSpace` callback. */
    virtual bool needsToReserveAllocationSpace() override {
        return true;
    }

    virtual void registerEHFrames(uint8_t* Addr, uint64_t LoadAddr, size_t Size) override {
        std::cout << "should register eh frames" << std::endl;
    }

    virtual void deregisterEHFrames() override {
        std::cout << "should deregister eh frames" << std::endl;
    }

    virtual bool finalizeMemory(std::string *ErrMsg = nullptr) override {
        auto code_result = callbacks.protect_memory(code_section.base, code_section.size, mem_protect_t::PROTECT_READ_EXECUTE);
        if (code_result != RESULT_OK) {
            return false;
        }

        auto read_result = callbacks.protect_memory(read_section.base, read_section.size, mem_protect_t::PROTECT_READ);
        if (read_result != RESULT_OK) {
            return false;
        }

        // The readwrite section is already mapped as read-write.

        return false;
    }

    virtual void notifyObjectLoaded(llvm::RuntimeDyld &RTDyld, const llvm::object::ObjectFile &Obj) override {}
private:
    struct Section {
        uint8_t* base;
        size_t size;
    };

    uint8_t* allocate_bump(Section& section, uintptr_t& bump_ptr, size_t size, size_t align) {
        auto aligner = [](uintptr_t& ptr, size_t align) {
            ptr = (ptr + align - 1) & ~(align - 1);
        };

        // Align the bump pointer to the requires alignment.
        aligner(bump_ptr, align);

        auto ret_ptr = bump_ptr;
        bump_ptr += size;

        assert(bump_ptr <= (uintptr_t)section.base + section.size);

        return (uint8_t*)ret_ptr;
    }

    Section code_section, read_section, readwrite_section;
    uintptr_t code_bump_ptr, read_bump_ptr, readwrite_bump_ptr;

    callbacks_t callbacks;
};

struct SymbolLookup : llvm::JITSymbolResolver {
public:
    virtual llvm::Expected<LookupResult> lookup(const LookupSet& symbols) override {
        LookupResult result;

        for (auto symbol : symbols) {
            result.emplace(symbol, symbol_lookup(symbol));
        }

        return result;
    }

    virtual llvm::Expected<LookupFlagsResult> lookupFlags(const LookupSet& symbols) override {
        LookupFlagsResult result;

        for (auto symbol : symbols) {
            result.emplace(symbol, symbol_lookup(symbol).getFlags());
        }

        return result;
    }

private:
    llvm::JITEvaluatedSymbol symbol_lookup(llvm::StringRef name) {
        std::cout << "symbol name: " << (std::string)name << std::endl;
        uint64_t addr = 0;

        return llvm::JITEvaluatedSymbol(addr, llvm::JITSymbolFlags::None);
    }
};

WasmModule::WasmModule(
        const uint8_t *object_start,
        size_t object_size,
        callbacks_t callbacks
) : memory_manager(new MemoryManager(callbacks))
{
    object_file = llvm::cantFail(llvm::object::ObjectFile::createObjectFile(llvm::MemoryBufferRef(
        llvm::StringRef((const char *)object_start, object_size), "object"
    )));

    SymbolLookup symbol_resolver;
    runtime_dyld = std::unique_ptr<llvm::RuntimeDyld>(new llvm::RuntimeDyld(*memory_manager, symbol_resolver));

    runtime_dyld->setProcessAllSections(true);

    runtime_dyld->loadObject(*object_file);
    runtime_dyld->finalizeWithMemoryManagerLocking();

    if (runtime_dyld->hasError()) {
        std::cout << "RuntimeDyld error: " << (std::string)runtime_dyld->getErrorString() << std::endl;
        abort();
    }
}

void* WasmModule::get_func(llvm::StringRef name) const {
    auto symbol = runtime_dyld->getSymbol(name);
    return (void*)symbol.getAddress();
}
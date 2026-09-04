#pragma once

#include <any>
#include <typeindex>
#include <unordered_map>

namespace voidui {

class TypeMap {
private:
  template <typename T> using Value = std::remove_cvref_t<T>;

  std::unordered_map<std::type_index, std::any> values_;

public:
  template <class T, class... Args> Value<T> &emplace(Args &&...args) {
    std::any value(std::in_place_type<Value<T>>, std::forward<Args>(args)...);

    auto [it, inserted] = values_.insert_or_assign(
        std::type_index(typeid(Value<T>)), std::move(value));

    return *std::any_cast<Value<T>>(&it->second);
  }

  template <typename T> Value<T> *get() noexcept {
    auto it = values_.find(std::type_index(typeid(Value<T>)));
    if (it == values_.end())
      return nullptr;

    return std::any_cast<Value<T>>(&it->second);
  }

  template <typename T> const Value<T> *get() const noexcept {
    auto it = values_.find(std::type_index(typeid(Value<T>)));
    if (it == values_.end())
      return nullptr;

    return std::any_cast<Value<T>>(&it->second);
  }

  template <class T> bool contains() const noexcept {
    return values_.contains(std::type_index(typeid(Value<T>))); // C++20
  }

  template <class T> bool erase() {
    using U = StoredType<T>;
    return values_.erase(std::type_index(typeid(Value<T>))) != 0;
  }
};

} // namespace voidui